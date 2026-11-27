use battery::Session;
use battery::adapter::{self, sh_ok};
use battery::common::restored;
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::tree;
use br_agentic_harness_contract::Seed;

const WORK_FILE: &str = "/work/agent-wrote-one.txt";
const WORK_TEXT: &str = "text written under work";
const HOME_FILE: &str = "/home/agent/agent-wrote-two.txt";
const HOME_TEXT: &str = "text written under home";

#[tokio::test(flavor = "multi_thread")]
async fn hard_links_and_the_executable_bit_survive_save_and_restore() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sh_ok(
        &sandbox,
        &opened.box_id,
        "mkdir -p /work/p && printf one > /work/p/a && ln /work/p/a /work/p/b && ln /work/p/a /work/p/c && printf x > /work/p/run.sh && chmod 0755 /work/p/run.sh && printf y > /work/p/plain && chmod 0644 /work/p/plain",
    )
    .await?;
    let before = tree::capture(&sandbox, &opened.box_id).await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    checks.expect(
        "properties-start-is-restored",
        restored(&again.start, first),
        format!("{:?}", again.start),
    );
    let after = tree::capture(&sandbox, &again.box_id).await?;
    let differences = tree::diff(&before, &after, 1);
    checks.expect(
        "hard-links-survive-save-and-restore",
        !differences
            .iter()
            .any(|d| d.contains("hard link") || d.contains("/work/p/")),
        differences.join(" | "),
    );
    let same_inode = sh_ok(&sandbox, &again.box_id, "[ \"$(stat -c %i /work/p/a)\" = \"$(stat -c %i /work/p/b)\" ] && [ \"$(stat -c %i /work/p/a)\" = \"$(stat -c %i /work/p/c)\" ] && [ \"$(stat -c %h /work/p/a)\" = 3 ] && echo linked").await?;
    checks.expect(
        "hard-link-group-is-one-inode-of-three-names",
        same_inode.trim() == "linked",
        same_inode,
    );
    let bits = tree::executable_bits(&after);
    checks.expect(
        "executable-bit-survives-save-and-restore",
        bits.get("/work/p/run.sh") == Some(&true) && bits.get("/work/p/plain") == Some(&false),
        format!(
            "{:?} {:?}",
            bits.get("/work/p/run.sh"),
            bits.get("/work/p/plain")
        ),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_by_the_agent_stays_deleted_after_the_next_restore() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let mut reaper = PodReaper::new(env);
    let lane = Lane::new()?;
    reaper.track_thread(lane.thread.to_string());
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sh_ok(
        &sandbox,
        &opened.box_id,
        &format!("printf '{WORK_TEXT}' > {WORK_FILE} && printf '{HOME_TEXT}' > {HOME_FILE}"),
    )
    .await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let (second, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    checks.expect(
        "deleted-file-case-second-start-is-restored",
        restored(&again.start, first),
        format!("{:?}", again.start),
    );
    let both = sh_ok(
        &sandbox,
        &again.box_id,
        &format!(
            "[ \"$(cat {WORK_FILE})\" = '{WORK_TEXT}' ] && [ \"$(cat {HOME_FILE})\" = '{HOME_TEXT}' ] && echo ok"
        ),
    )
    .await
    .map_err(|e| data("files before the delete", e.to_string()))?;
    checks.expect(
        "both-files-written-by-the-agent-are-restored-before-the-delete",
        both.trim() == "ok",
        both,
    );
    sh_ok(&sandbox, &again.box_id, &format!("rm -f {WORK_FILE}")).await?;
    adapter::save(&sandbox, &again.box_id).await?;
    adapter::release(&sandbox, &again.box_id).await?;
    let (_, last) = lane.child(second)?;
    let third = adapter::open(&sandbox, last, Seed::empty()).await?;
    checks.expect(
        "deleted-file-case-third-start-is-restored",
        restored(&third.start, second),
        format!("{:?}", third.start),
    );
    let state = sh_ok(
        &sandbox,
        &third.box_id,
        &format!("[ ! -e {WORK_FILE} ] && [ \"$(cat {HOME_FILE})\" = '{HOME_TEXT}' ] && echo ok"),
    )
    .await
    .map_err(|e| data("deleted file check", e.to_string()))?;
    checks.expect(
        "file-deleted-by-the-agent-stays-deleted-and-the-other-file-is-still-there",
        state.trim() == "ok",
        format!("deleted {WORK_FILE}, kept {HOME_FILE}: {state}"),
    );
    adapter::release(&sandbox, &third.box_id).await?;
    checks.finish()
}
