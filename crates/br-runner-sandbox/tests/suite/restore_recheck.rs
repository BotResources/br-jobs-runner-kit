use std::time::Duration;

use br_agentic_harness_contract::FreshReason;
use br_runner_sandbox::{RestoreVerdict, live_expired, manifest_key};

use crate::fakes::{POD, Script, rig, start_time};
use crate::flow_support::stage;

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_manifest_that_is_gone_after_the_parts_makes_the_save_expired_with_the_first_time()
 {
    let rig = rig();
    let staged = stage(&rig);
    let key = staged.manifest.key().clone();
    rig.cluster.push_script(Script::like_bsdtar_extract());
    let store = rig.store.clone();
    rig.store
        .on_stream(move || store.remove(&manifest_key(&key)));
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(live_expired(start_time().plus(rig.config.save_ttl())))
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_that_expires_during_the_restore_gives_save_expired_with_the_first_time()
 {
    let rig = rig();
    let staged = stage(&rig);
    rig.cluster.push_script(Script::like_bsdtar_extract());
    let clock = rig.clock.clone();
    let ttl = rig.config.save_ttl();
    rig.store
        .on_stream(move || clock.advance(ttl + Duration::from_secs(1)));
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(live_expired(start_time().plus(ttl)))
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_failing_second_read_of_the_manifest_is_a_store_that_is_unavailable()
 {
    let rig = rig();
    let staged = stage(&rig);
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.store.fail("get", "manifest.json");
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(FreshReason::StoreUnavailable)
    );
}
