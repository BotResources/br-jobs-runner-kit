use std::time::Instant;

use battery::Session;
use battery::adapter::Tunables;
use battery::common::pending_uploads;
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::prng::Prng;
use battery::proc::{self, strings};
use battery::saves::Saves;
use br_runner_sandbox::{PutOutcome, S3Store, Store, StoreError};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use uuid::Uuid;

const UNFINISHED_CHUNKS: usize = 12;

fn probe_prefix(label: &str) -> String {
    format!("battery-probe/{label}-{}", Uuid::now_v7())
}

#[tokio::test(flavor = "multi_thread")]
async fn conditional_create_refuses_a_second_writer() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    for round in 0..5 {
        let key = format!("{}/manifest-{round}", probe_prefix("cas"));
        let (first, second) = tokio::join!(
            saves.store.put_if_absent(&key, Bytes::from_static(b"one")),
            saves.store.put_if_absent(&key, Bytes::from_static(b"two"))
        );
        let verdict = matches!(
            (&first, &second),
            (Ok(PutOutcome::Created), Ok(PutOutcome::AlreadyExists))
                | (Ok(PutOutcome::AlreadyExists), Ok(PutOutcome::Created))
        );
        let unsupported = matches!(first, Err(StoreError::ConditionalCreateUnsupported))
            || matches!(second, Err(StoreError::ConditionalCreateUnsupported));
        checks.expect(
            &format!("store-conditional-create-round-{round}"),
            verdict,
            format!("{first:?} and {second:?}; unsupported answer: {unsupported}"),
        );
        saves.store.delete(&key).await?;
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn multipart_part_has_its_size_and_abort_leaves_nothing() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let prefix = probe_prefix("multipart");
    let key = format!("{prefix}/part");
    let mut prng = Prng::new(5);
    let mut upload = saves.store.start_upload(&key).await?;
    let mut written = 0u64;
    for _ in 0..50 {
        let chunk = prng.bytes(1_000_000);
        written += chunk.len() as u64;
        upload.write(Bytes::from(chunk)).await?;
    }
    upload.finish().await?;
    let size = saves.store.size(&key).await?;
    checks.expect(
        "store-multipart-size-read-back",
        size == Some(written),
        format!("written {written}, read back {size:?}"),
    );
    let control = format!("{prefix}/control");
    let mut open_upload = saves.store.start_upload(&control).await?;
    for _ in 0..UNFINISHED_CHUNKS {
        open_upload
            .write(Bytes::from(prng.bytes(1_000_000)))
            .await?;
    }
    let control_pending = pending_uploads(env, &control).await?;
    checks.expect(
        "store-multipart-an-unfinished-upload-shows-as-pending",
        control_pending == 1,
        format!("{control_pending} pending uploads for an unfinished upload"),
    );
    open_upload.abort().await?;
    let aborted = format!("{prefix}/aborted");
    let mut upload = saves.store.start_upload(&aborted).await?;
    for _ in 0..UNFINISHED_CHUNKS {
        upload.write(Bytes::from(prng.bytes(1_000_000))).await?;
    }
    upload.abort().await?;
    let gone = saves.store.size(&aborted).await?.is_none();
    let pending = pending_uploads(env, &prefix).await?;
    checks.expect(
        "store-multipart-abort-leaves-nothing",
        gone && pending == 0,
        format!("object gone {gone}, pending uploads {pending}"),
    );
    saves.store.delete(&key).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn list_delete_and_path_style_work_on_the_real_store() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let mut forms = Vec::new();
    for (name, style, endpoint) in [
        ("path-style", true, env.s3.endpoint.clone()),
        (
            "virtual-host-style",
            false,
            env.s3
                .endpoint
                .replace("127.0.0.1", &format!("{}.localhost", env.s3.bucket)),
        ),
    ] {
        let mut tunables = Tunables::standard(env);
        tunables.store_path_style = style;
        tunables.store_endpoint = endpoint;
        forms.push((name, Saves::through(env, &tunables)?));
    }
    for (name, saves) in &forms {
        let prefix = probe_prefix(name);
        let puts = stream::iter(0..1500)
            .map(|index| {
                let key = format!("{prefix}/k{index:04}");
                async move { saves.store.put(&key, Bytes::from_static(b"x")).await }
            })
            .buffer_unordered(32)
            .collect::<Vec<_>>()
            .await;
        let put_errors = puts.iter().filter(|result| result.is_err()).count();
        if put_errors > 0 {
            checks.expect(
                &format!("store-{name}-reaches-the-bucket"),
                false,
                format!(
                    "{put_errors} of 1500 puts failed: {:?}",
                    puts.iter().find(|r| r.is_err())
                ),
            );
            continue;
        }
        let mut keys = saves.store.list(&format!("{prefix}/")).await?;
        let listed = keys.len();
        keys.sort();
        keys.dedup();
        checks.expect(
            &format!("store-{name}-lists-every-key-once"),
            listed == 1500 && keys.len() == 1500,
            format!("listed {listed} keys, {} distinct, of 1500", keys.len()),
        );
        let first = format!("{prefix}/k0000");
        let deleted = saves.store.delete(&first).await;
        let missing = saves.store.delete(&format!("{prefix}/never-there")).await;
        let rest = saves.store.list(&format!("{prefix}/")).await?;
        checks.expect(
            &format!("store-{name}-delete-of-present-and-missing-key"),
            deleted.is_ok() && missing.is_ok() && rest.len() == 1499 && !rest.contains(&first),
            format!(
                "{deleted:?} and {missing:?}, {} keys left, first still listed {}",
                rest.len(),
                rest.contains(&first)
            ),
        );
        for key in saves.store.list(&format!("{prefix}/")).await? {
            saves.store.delete(&key).await?;
        }
    }
    checks.finish()
}

fn tls_failure(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "certificate",
        "tls",
        "handshake",
        "unknown issuer",
        "invalid peer",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

fn server_answered(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "403",
        "401",
        "404",
        "accessdenied",
        "invalidaccesskeyid",
        "signaturedoesnotmatch",
        "forbidden",
        "unauthorized",
        "nosuchbucket",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

async fn store_at(env: &battery::env::Env, endpoint: &str) -> Res<S3Store> {
    let mut tunables = Tunables::standard(env);
    tunables.store_endpoint = endpoint.to_owned();
    tunables.store_path_style = true;
    let config = tunables.config(env)?;
    Ok(S3Store::new(config.store())?)
}

#[tokio::test(flavor = "multi_thread")]
async fn store_client_trusts_system_roots_only() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let trusted = battery::env::optional("TLS_TRUSTED_ENDPOINT")
        .unwrap_or_else(|| "https://s3.amazonaws.com".to_owned());
    let good = store_at(env, &trusted).await?;
    let answer = good.get("battery-probe/none").await;
    let text = format!("{answer:?}");
    checks.expect(
        "store-tls-system-root-is-trusted",
        !tls_failure(&text) && (answer.is_ok() || server_answered(&text)),
        text.clone(),
    );
    std::fs::create_dir_all(&env.work)?;
    let cert = env.work.join("battery-unknown.crt");
    let key = env.work.join("battery-unknown.key");
    let made = proc::run(
        "openssl",
        &strings(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
            "-keyout",
            &key.display().to_string(),
            "-out",
            &cert.display().to_string(),
        ]),
        None,
        proc::SHORT,
    )
    .await?;
    proc::succeeded("openssl", &[], made)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let mut server = tokio::process::Command::new("openssl")
        .args(["s_server", "-www", "-accept", &port.to_string(), "-cert"])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| data("openssl s_server", error.to_string()))?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let started = Instant::now();
    let bad = store_at(env, &format!("https://127.0.0.1:{port}")).await?;
    let answer = bad.get("battery-probe/none").await;
    let text = format!("{answer:?}");
    let leaks = text.contains(&env.s3.secret_key) || text.contains(&env.s3.access_key);
    checks.expect(
        "store-tls-unknown-root-is-refused-with-no-credential",
        answer.is_err() && tls_failure(&text) && !leaks,
        format!("after {:?}: {text}", started.elapsed()),
    );
    let _ = server.kill().await;
    let _ = std::fs::remove_file(cert);
    let _ = std::fs::remove_file(key);
    checks.finish()
}
