use battery::Session;
use battery::adapter::{self, sh_ok};
use battery::common::restored;
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::proc::{self, SHORT};
use br_agentic_harness_contract::{SandboxPort, Seed};
use br_runner_sandbox::{Sandbox, SandboxConfig, SandboxConfigInput};
use serde_json::{Value, json};

const KNOWN_KEYS: [&str; 27] = [
    "namespace",
    "kube_context",
    "client_id",
    "label_prefix",
    "pool_id",
    "network_mode",
    "allowlist",
    "save_ttl_seconds",
    "save_scope_paths",
    "save_zstd_level",
    "restore_timeout_seconds",
    "open_timeout_seconds",
    "exec_timeout_seconds",
    "save_timeout_seconds",
    "idle_ttl_seconds",
    "idle_keep_min_state_bytes",
    "claim_max_attempts",
    "cold_create",
    "agent_container",
    "seed_container",
    "tools_path",
    "seed_tools_path",
    "seed_root",
    "store_endpoint",
    "store_bucket",
    "store_region",
    "store_path_style",
];

async fn document_as_runner(session: &Session) -> Res<Value> {
    document_of(session, &session.env.namespace()).await
}

async fn document_of(session: &Session, namespace: &str) -> Res<Value> {
    let env = &session.env;
    let identity = format!(
        "--as=system:serviceaccount:{}:{}",
        env.runner_ns, env.runner_sa
    );
    let namespace = namespace.to_owned();
    let args = vec![
        "--context".to_owned(),
        env.context.clone(),
        identity,
        "-n".to_owned(),
        namespace,
        "get".to_owned(),
        "configmap".to_owned(),
        "br-sandbox-config".to_owned(),
        "-o".to_owned(),
        "jsonpath={.data.config\\.json}".to_owned(),
    ];
    let output = proc::run("kubectl", &args, None, SHORT).await?;
    let output = proc::succeeded("kubectl", &args, output)?;
    serde_json::from_slice(&output.stdout)
        .map_err(|error| data("the adapter document", error.to_string()))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_document_of_the_chart_builds_the_adapter_and_runs_one_flow() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let mut document = document_as_runner(&session).await?;
    let keys: Vec<String> = document
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    checks.expect(
        "document-read-by-the-runner-identity",
        document.is_object(),
        keys.join(","),
    );
    let outside: Vec<&String> = keys
        .iter()
        .filter(|key| !KNOWN_KEYS.contains(&key.as_str()))
        .collect();
    checks.expect(
        "document-has-only-keys-of-the-adapter-input",
        outside.is_empty(),
        format!("keys outside the input type: {outside:?}"),
    );
    checks.expect(
        "document-holds-no-credential",
        !keys.iter().any(|key| key == "store_credentials"),
        keys.join(","),
    );
    let agent = document["agent_container"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    checks.expect(
        "document-names-the-agent-container-of-the-pod",
        agent == env.agent_container,
        format!("document {agent:?}, pod {:?}", env.agent_container),
    );
    let mut with_extra = document.clone();
    with_extra["an_unknown_key"] = json!(1);
    with_extra["store_credentials"] = json!({"access_key_id": "a", "secret_access_key": "b"});
    let extra: Result<SandboxConfigInput, _> = serde_json::from_value(with_extra);
    checks.expect(
        "document-with-an-unknown-key-is-accepted-or-refused-by-the-input-type",
        true,
        match &extra {
            Ok(_) => "accepted".to_owned(),
            Err(error) => format!("refused: {error}"),
        },
    );
    document["store_credentials"] = json!({
        "access_key_id": env.s3.access_key,
        "secret_access_key": env.s3.secret_key,
    });
    document["kube_context"] = json!(env.context);
    let written_endpoint = document["store_endpoint"].as_str().unwrap_or("").to_owned();
    checks.expect(
        "document-store-endpoint-is-the-chart-value-and-the-host-reaches-the-store-by-another-address",
        written_endpoint == "http://store.sbx-teststore.svc.cluster.local:8333",
        format!("document {written_endpoint:?}, host address {:?}", env.s3.endpoint),
    );
    document["store_endpoint"] = json!(env.s3.endpoint);
    let input: SandboxConfigInput = match serde_json::from_value(document) {
        Ok(input) => input,
        Err(error) => {
            checks.expect(
                "document-deserializes-through-the-input-type",
                false,
                error.to_string(),
            );
            return checks.finish();
        }
    };
    checks.expect("document-deserializes-through-the-input-type", true, "ok");
    let config = match SandboxConfig::try_from(input) {
        Ok(config) => config,
        Err(error) => {
            checks.expect(
                "document-validates-as-the-adapter-config",
                false,
                error.to_string(),
            );
            return checks.finish();
        }
    };
    checks.expect("document-validates-as-the-adapter-config", true, "ok");
    let sandbox = match Sandbox::connect(config).await {
        Ok(sandbox) => sandbox,
        Err(error) => {
            checks.expect("adapter-connects", false, error.to_string());
            return checks.finish();
        }
    };
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let user = sh_ok(&sandbox, &opened.box_id, "id -u").await?;
    checks.expect(
        "flow-command-runs-as-the-agent-user",
        !user.trim().is_empty() && user.trim() != "0",
        user.trim(),
    );
    sandbox
        .write(
            &opened.box_id,
            &adapter::path("document-flow.txt")?,
            b"from the chart document".to_vec(),
        )
        .await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    checks.expect(
        "flow-second-open-is-restored",
        restored(&again.start, first),
        format!("{:?}", again.start),
    );
    let back = sandbox
        .read(&again.box_id, &adapter::path("document-flow.txt")?)
        .await?;
    checks.expect(
        "flow-file-is-back-after-restore",
        back == b"from the chart document".to_vec(),
        String::from_utf8_lossy(&back),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_document_of_every_sandbox_namespace_builds_the_adapter_config() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let label = format!(
        "{}=true",
        battery::env::required("LBL_MANAGED", battery::env::FILE_CHART_NAMES)?
    );
    let listed = proc::Kube::cluster_wide(env)
        .run(&[
            "get",
            "namespaces",
            "-l",
            &label,
            "-o",
            "jsonpath={.items[*].metadata.name}",
        ])
        .await?;
    let namespaces: Vec<&str> = listed.split_whitespace().collect();
    checks.expect(
        "every-sandbox-namespace-is-listed",
        !namespaces.is_empty(),
        format!("{namespaces:?}"),
    );
    for namespace in namespaces {
        let mut document = document_of(&session, namespace).await?;
        document["store_credentials"] = json!({
            "access_key_id": env.s3.access_key,
            "secret_access_key": env.s3.secret_key,
        });
        let mode = document["network_mode"].as_str().unwrap_or("").to_owned();
        let allowlist = document["allowlist"].to_string();
        let built = serde_json::from_value::<SandboxConfigInput>(document)
            .map_err(|error| error.to_string())
            .and_then(|input| {
                SandboxConfig::try_from(input)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        checks.expect(
            &format!("document-of-{namespace}-builds-the-adapter-config"),
            built.is_ok(),
            format!("mode {mode}, allowlist {allowlist}, {built:?}"),
        );
    }
    checks.finish()
}
