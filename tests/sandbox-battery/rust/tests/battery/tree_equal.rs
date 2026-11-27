use std::time::Instant;

use battery::Session;
use battery::adapter::{self, background, sh_ok};
use battery::common::restored;
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::tree;
use br_agentic_harness_contract::Seed;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tree_of_1000_files_and_one_50_mb_file_is_equal_after_restore() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    tree::build(&sandbox, &opened.box_id, 2026).await?;
    background(&sandbox, &opened.box_id, "sleep 3001").await?;
    let before = tree::capture(&sandbox, &opened.box_id).await?;
    let kinds = |kind: &str| before.entries.values().filter(|e| e.kind == kind).count();
    checks.expect(
        "tree-case-is-complete",
        kinds("l") >= 20 && kinds("d") >= 20 && before.hashes.len() >= 1001,
        format!(
            "{} links, {} directories, {} hashed files",
            kinds("l"),
            kinds("d"),
            before.hashes.len()
        ),
    );
    let started = Instant::now();
    let receipt = adapter::save(&sandbox, &opened.box_id).await?;
    env.record(
        "tree_save_seconds",
        format!("{:.2}", started.elapsed().as_secs_f64()),
        "s",
    );
    env.record("tree_save_raw_bytes", receipt.bytes, "bytes");
    adapter::release(&sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let started = Instant::now();
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    env.record(
        "tree_restore_seconds",
        format!("{:.2}", started.elapsed().as_secs_f64()),
        "s",
    );
    checks.expect(
        "tree-start-is-restored",
        restored(&again.start, first),
        format!("{:?}", again.start),
    );
    let after = tree::capture(&sandbox, &again.box_id).await?;
    let differences = tree::diff(&before, &after, 1);
    env.record("tree_differences", differences.len(), "count");
    checks.expect(
        "tree-equal-after-restore-zero-differences",
        differences.is_empty(),
        differences
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | "),
    );
    checks.expect(
        "tree-executable-bits-equal",
        tree::executable_bits(&before) == tree::executable_bits(&after),
        "executable bits",
    );
    let ids = after
        .entries
        .values()
        .map(|e| (e.uid.clone(), e.gid.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    checks.expect(
        "tree-owner-ids-are-the-agent-ids-only",
        ids.len() == 1,
        format!("{ids:?}"),
    );
    let tmp = sh_ok(
        &sandbox,
        &again.box_id,
        "[ ! -e /tmp/not-kept.txt ] && echo gone",
    )
    .await?;
    let running = sh_ok(&sandbox, &again.box_id, "for f in /proc/[0-9]*/cmdline; do tr '\\0' ' ' < $f; echo; done 2>/dev/null | grep -c '[s]leep 3001' || true").await?;
    checks.expect("tree-tmp-is-not-restored", tmp.trim() == "gone", tmp);
    checks.expect(
        "tree-background-process-is-not-restored",
        running.trim() == "0",
        running,
    );
    adapter::release(&sandbox, &again.box_id).await?;
    checks.finish()
}
