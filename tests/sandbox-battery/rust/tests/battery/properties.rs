use battery::Session;
use battery::adapter::{self, sh_ok};
use battery::common::restored;
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::tree;
use br_agentic_harness_contract::Seed;

const IMAGE_FILES: [&str; 2] = ["/home/agent/image-two.txt", "/work/image-one.txt"];

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
async fn an_image_file_deleted_by_the_agent_comes_back_only_if_it_is_in_the_save() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let listing = sh_ok(
        &sandbox,
        &opened.box_id,
        "find /work /home/agent -mindepth 1 -type f | sort",
    )
    .await?;
    let files: Vec<&str> = listing.lines().collect();
    if files != IMAGE_FILES {
        checks.expect(
            "image-file-deleted-by-the-agent-stays-deleted",
            false,
            format!(
                "the pool image must hold exactly {IMAGE_FILES:?} in the save scope (/work, /home/agent) and holds {files:?}"
            ),
        );
        adapter::release(&sandbox, &opened.box_id).await?;
        return checks.finish();
    }
    let (gone, kept) = (files[0].to_owned(), files[1].to_owned());
    sh_ok(&sandbox, &opened.box_id, &format!("rm -f '{gone}'")).await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    checks.expect(
        "deleted-image-file-case-start-is-restored",
        restored(&again.start, first),
        format!("{:?}", again.start),
    );
    let state = sh_ok(
        &sandbox,
        &again.box_id,
        &format!("[ ! -e '{gone}' ] && [ -e '{kept}' ] && echo ok"),
    )
    .await
    .map_err(|e| data("deleted file check", e.to_string()))?;
    checks.expect(
        "image-file-deleted-by-the-agent-stays-deleted",
        state.trim() == "ok",
        format!("deleted {gone}, kept {kept}: {state}"),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    checks.finish()
}
