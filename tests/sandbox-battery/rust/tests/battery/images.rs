use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, path, request, sh_ok};
use battery::common::{restored, wait_until};
use battery::env::Env;
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::guards::ValueGuard;
use battery::ids::Lane;
use battery::pool;
use battery::proc::{self, Helm, strings};
use br_agentic_harness_contract::{SandboxPort, Seed};
use br_runner_sandbox::{Error, ImageError, Sandbox};
use serde_json::{Value, json};

async fn the_program(sandbox: &Sandbox, version_command: &str) -> Res<String> {
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(sandbox, turn, Seed::empty()).await?;
    sandbox
        .write(&opened.box_id, &path("f")?, b"program".to_vec())
        .await?;
    let version = sh_ok(sandbox, &opened.box_id, version_command).await?;
    adapter::save(sandbox, &opened.box_id).await?;
    adapter::release(sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(sandbox, next, Seed::empty()).await?;
    let back = sandbox.read(&again.box_id, &path("f")?).await?;
    adapter::release(sandbox, &again.box_id).await?;
    if !restored(&again.start, first) || back != b"program" {
        return Err(data(
            "the program",
            format!("start {:?}, file {back:?}", again.start),
        ));
    }
    Ok(version)
}

async fn refresh_pool(env: &Env) -> Res {
    for name in pool::warm_list(env, false).await? {
        pool::delete_now(env, &name).await?;
    }
    pool::wait_warm(env, 1, 600).await
}

fn without_image(values: &Value, key: &str) -> Value {
    let mut copy = values.clone();
    if let Some(map) = copy.as_object_mut() {
        map.remove(key);
    }
    copy
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_image_comes_by_a_change_of_the_image_value_only() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let second = env.named("SECOND_IMAGE")?;
    let first_command = env
        .named("FIRST_VERSION_COMMAND")
        .unwrap_or_else(|_| "python3 --version".to_owned());
    let second_command = env
        .named("SECOND_VERSION_COMMAND")
        .unwrap_or_else(|_| "R --version".to_owned());
    let mut checks = Checks::new(env);
    let helm = Helm::of(env);
    let sandbox = adapter::standard(env).await?;
    let before = helm.values().await?;
    let one = the_program(&sandbox, &first_command).await?;
    checks.expect(
        "two-images-first-run-passes",
        !one.trim().is_empty(),
        one.trim(),
    );
    let mut guard = ValueGuard::new(env);
    let started = Instant::now();
    guard.set(&[(env.image_key.clone(), json!(second))]).await?;
    refresh_pool(env).await?;
    env.record(
        "second_image_pool_warm_seconds",
        format!("{:.1}", started.elapsed().as_secs_f64()),
        "s",
    );
    let two = the_program(&sandbox, &second_command).await?;
    checks.expect(
        "two-images-second-run-passes",
        !two.trim().is_empty(),
        two.trim(),
    );
    let after = helm.values().await?;
    checks.expect(
        "two-images-only-the-image-value-changed",
        without_image(&before, &env.image_key) == without_image(&after, &env.image_key)
            && before[&env.image_key] != after[&env.image_key],
        format!(
            "image {} to {}",
            before[&env.image_key], after[&env.image_key]
        ),
    );
    env.record("two_images_values_before", before.to_string(), "json");
    env.record("two_images_values_after", after.to_string(), "json");
    drop(guard);
    refresh_pool(env).await?;
    checks.finish()
}

async fn build_and_push(base: &str, push: &str, version: u32) -> Res {
    let dockerfile = format!("FROM {base}\nRUN echo {version} > /etc/image-version\n");
    let args = strings(&["build", "--no-cache", "-t", push, "-"]);
    let built = proc::run("docker", &args, Some(dockerfile.into_bytes()), proc::LONG).await?;
    proc::succeeded("docker", &args, built)?;
    let args = strings(&["push", push]);
    let pushed = proc::run("docker", &args, None, proc::LONG).await?;
    proc::succeeded("docker", &args, pushed)?;
    Ok(())
}

async fn read_version(sandbox: &Sandbox) -> Res<(String, String)> {
    let opened = adapter::open(sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
    let version = sh_ok(sandbox, &opened.box_id, "cat /etc/image-version").await?;
    adapter::release(sandbox, &opened.box_id).await?;
    Ok((version.trim().to_owned(), opened.box_id.to_string()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_push_under_the_same_tag_reaches_the_pool_within_the_warm_max_age() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let image = env.named("MOVING_IMAGE")?;
    let push = env.named("MOVING_PUSH")?;
    let base = env.named("BASE_IMAGE")?;
    let mut checks = Checks::new(env);
    let mut guard = ValueGuard::new(env);
    let limit = 120u64;
    build_and_push(&base, &push, 1).await?;
    guard
        .set(&[
            (env.image_key.clone(), json!(image)),
            (env.warm_max_age_key.clone(), json!(limit)),
            ("image_pull_policy".to_owned(), json!("")),
            ("network_check.run_id".to_owned(), json!("moving")),
        ])
        .await?;
    refresh_pool(env).await?;
    let sandbox = adapter::standard(env).await?;
    let warm = pool::warm_list(env, true).await?;
    let old = pool::pod(env, &warm[0])
        .await?
        .ok_or_else(|| data("warm pod", "absent"))?;
    let policy = old["spec"]["containers"][0]["imagePullPolicy"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    checks.expect(
        "moving-tag-pull-policy-is-always",
        policy == "Always",
        policy,
    );
    let old_id = old["status"]["containerStatuses"][0]["imageID"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let (first, _) = read_version(&sandbox).await?;
    checks.expect("moving-tag-first-box-reads-1", first == "1", first);
    build_and_push(&base, &push, 2).await?;
    let pushed = Instant::now();
    let mut new_id = String::new();
    let mut warm_delay = None;
    while pushed.elapsed() < Duration::from_secs(limit + 120) {
        for name in pool::warm_list(env, true).await? {
            if let Some(pod) = pool::pod(env, &name).await? {
                let id = pod["status"]["containerStatuses"][0]["imageID"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                if !id.is_empty() && id != old_id {
                    new_id = id;
                }
            }
        }
        if !new_id.is_empty() {
            warm_delay = Some(pushed.elapsed().as_secs_f64());
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    env.record(
        "moving_tag_delay_to_new_warm_pod",
        warm_delay.map_or_else(|| "never".to_owned(), |d| format!("{d:.0}")),
        "s",
    );
    env.record("moving_tag_warm_max_age_seconds", limit, "s");
    checks.expect(
        "moving-tag-new-warm-pod-within-warm-max-age",
        warm_delay.is_some_and(|d| d <= limit as f64),
        format!("{warm_delay:?} against {limit}"),
    );
    checks.expect(
        "moving-tag-new-warm-pods-have-another-image-id",
        !new_id.is_empty() && new_id != old_id,
        format!("{old_id} then {new_id}"),
    );
    let reading = Instant::now();
    let mut seen = None;
    while reading.elapsed() < Duration::from_secs(limit) {
        let (version, name) = read_version(&sandbox).await?;
        if version == "2" {
            seen = Some(name);
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    checks.expect(
        "moving-tag-a-box-opened-after-the-new-warm-pod-reads-the-new-content",
        seen.is_some(),
        format!("{seen:?}"),
    );
    if let Some(name) = seen {
        let gone = pool::pod(env, &name)
            .await?
            .is_none_or(|p| pool::label(&p, "br.sandbox/pool").as_deref() != Some("warm"));
        checks.expect(
            "moving-tag-a-claimed-pod-never-goes-back-to-the-pool",
            gone,
            name,
        );
    }
    drop(guard);
    refresh_pool(env).await?;
    pool::replace_warm_with_image(env, &image, 600).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_image_that_starts_and_crashes_at_once_gives_image_failing() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let crashing = env.named("CRASHING_IMAGE")?;
    let mut checks = Checks::new(env);
    let mut guard = ValueGuard::new(env);
    let mut tunables = Tunables::standard(env);
    tunables.open_timeout = 20;
    let sandbox = tunables.connect(env).await?;
    let started = Instant::now();
    guard
        .set(&[(env.image_key.clone(), json!(crashing))])
        .await?;
    for name in pool::warm_list(env, false).await? {
        pool::delete_now(env, &name).await?;
    }
    wait_until("the condition WarmPodsFailing", 300, || async {
        Ok(pool::replica_set(env).await?["status"]["conditions"]
            .as_array()
            .is_some_and(|all| {
                all.iter()
                    .any(|c| c["type"] == "WarmPodsFailing" && c["status"] == "True")
            }))
    })
    .await?;
    env.record(
        "image_crash_upgrade_to_condition_seconds",
        format!("{:.1}", started.elapsed().as_secs_f64()),
        "s",
    );
    let began = Instant::now();
    let outcome = sandbox
        .open_box(&request(Lane::new()?.first()?.1, Seed::empty(), None)?)
        .await;
    env.record(
        "image_crash_upgrade_to_error_seconds",
        format!("{:.1}", started.elapsed().as_secs_f64()),
        "s",
    );
    checks.expect(
        "image-crash-gives-image-failing-not-unavailable",
        matches!(&outcome, Err(Error::Image(ImageError::Failing { .. })))
            && began.elapsed().as_secs() < 20,
        format!("{outcome:?}"),
    );
    drop(guard);
    refresh_pool(env).await?;
    let again = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await;
    checks.expect(
        "image-crash-condition-clears-and-open-works-after-the-good-image",
        again.is_ok(),
        format!("{:?}", again.as_ref().map(|o| o.start)),
    );
    if let Ok(opened) = again {
        adapter::release(&sandbox, &opened.box_id).await?;
    }
    checks.finish()
}
