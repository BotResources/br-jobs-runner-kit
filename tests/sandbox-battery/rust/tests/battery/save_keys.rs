use battery::Session;
use battery::adapter::{self, sh_ok};
use battery::common::restored;
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::{self, Lane};
use battery::saves::Saves;
use br_agentic_harness_contract::{AlreadySaved, SandboxPort, Seed};
use br_runner_sandbox::{Error, Store, manifest_key};
use sha2::{Digest, Sha256};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saves_that_differ_in_thread_agent_or_turn_are_distinct_objects() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let (th1, th2) = (Lane::new()?, Lane::new()?);
    let a2 = th1.with_agent()?;
    let (k1, k2) = (ids::turn()?, ids::turn()?);
    let cases = [
        ("th1-a1-k1", th1, k1),
        (
            "th2-a1-k1",
            Lane {
                thread: th2.thread,
                agent: th1.agent,
            },
            k1,
        ),
        ("th1-a2-k1", a2, k1),
        ("th1-a1-k2", th1, k2),
    ];
    for (name, lane, turn) in &cases {
        let opened = adapter::open(&sandbox, lane.turn(*turn, None), Seed::empty()).await?;
        sh_ok(
            &sandbox,
            &opened.box_id,
            &format!("printf {name} > /work/marker"),
        )
        .await?;
        adapter::save(&sandbox, &opened.box_id).await?;
        adapter::release(&sandbox, &opened.box_id).await?;
        battery::common::settle().await?;
    }
    let mut keys = Vec::new();
    for (name, lane, turn) in &cases {
        let key = saves.key(lane.thread, lane.agent, *turn);
        let present = saves.manifest(&key).await?.is_some();
        keys.push(manifest_key(&key));
        checks.expect(
            &format!("save-keys-{name}-has-its-manifest"),
            present,
            manifest_key(&key),
        );
        let (_, next) = lane.child(*turn)?;
        let again = adapter::open(&sandbox, next, Seed::empty()).await?;
        let marker = sh_ok(&sandbox, &again.box_id, "cat /work/marker").await?;
        checks.expect(
            &format!("save-keys-{name}-restores-only-its-own-content"),
            restored(&again.start, *turn) && marker == *name,
            format!("{:?} {marker}", again.start),
        );
        adapter::release(&sandbox, &again.box_id).await?;
        battery::common::settle().await?;
    }
    keys.sort();
    keys.dedup();
    checks.expect(
        "save-keys-four-different-keys",
        keys.len() == 4,
        format!("{}", keys.len()),
    );
    let key = saves.key(th1.thread, th1.agent, k1);
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let manifest_before = saves
        .store
        .get(&manifest_key(&key))
        .await?
        .map(|b| digest(&b));
    let part = saves.part_of(&key).await?;
    let part_before = saves.store.get(&part).await?.map(|b| digest(&b));
    let second = adapter::open(&sandbox, th1.turn(k1, None), Seed::empty()).await?;
    sh_ok(&sandbox, &second.box_id, "printf other > /work/marker").await?;
    let refused = sandbox.save(&second.box_id).await;
    let typed = sandbox.save_box(&second.box_id).await;
    checks.expect(
        "save-keys-same-key-twice-is-already-saved-by-the-port",
        matches!(&refused, Err(e) if AlreadySaved::is(e)),
        format!("{refused:?}"),
    );
    checks.expect(
        "save-keys-same-key-twice-is-already-saved-by-the-crate",
        matches!(typed, Err(Error::AlreadySaved)),
        "typed error",
    );
    let manifest_after = saves
        .store
        .get(&manifest_key(&key))
        .await?
        .map(|b| digest(&b));
    let part_after = saves.store.get(&part).await?.map(|b| digest(&b));
    checks.expect(
        "save-keys-first-save-is-unchanged",
        manifest_before == manifest_after && part_before == part_after,
        "manifest and part hashes",
    );
    adapter::release(&sandbox, &second.box_id).await?;
    battery::common::settle().await?;
    checks.finish()
}
