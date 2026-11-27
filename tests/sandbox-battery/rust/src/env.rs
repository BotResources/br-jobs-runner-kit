use std::path::PathBuf;

use crate::error::{BatteryError, Res};

pub const FILE_NAMES: &str = "tests/sandbox-battery/names.env";
pub const FILE_CHART_NAMES: &str = "tests/sandbox-battery/scenarios/names-chart.env";
pub const FILE_CREDS: &str = "the creds.env file of the set-up";
pub const FILE_README: &str = "the README of tests/sandbox-battery/rust";

#[derive(Debug, Clone)]
pub struct S3Env {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub path_style: bool,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone)]
pub struct Env {
    pub context: String,
    pub node: String,
    pub ns_prefix: String,
    pub client: String,
    pub mode: String,
    pub rs_prefix: String,
    pub agent_container: String,
    pub seed_container: String,
    pub seed_root: String,
    pub agent_tools: String,
    pub seed_tools: String,
    pub python: String,
    pub helm_release: String,
    pub helm_namespace: String,
    pub system_ns: String,
    pub keeper_deploy: String,
    pub runner_ns: String,
    pub runner_sa: String,
    pub image_key: String,
    pub warm_max_age_key: String,
    pub s3: S3Env,
    pub evidence: PathBuf,
    pub work: PathBuf,
    pub timing_lock: PathBuf,
    pub chart_dir: PathBuf,
    pub values_file: PathBuf,
}

pub fn required(name: &'static str, origin: &'static str) -> Res<String> {
    std::env::var(name).map_err(|_| BatteryError::MissingVariable { name, origin })
}

pub fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

pub fn flag(name: &'static str, origin: &'static str) -> Res<bool> {
    let value = required(name, origin)?;
    match value.as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(BatteryError::BadVariable {
            name,
            value,
            expected: "true or false",
        }),
    }
}

fn home() -> Res<PathBuf> {
    required("HOME", "the shell").map(PathBuf::from)
}

impl Env {
    pub fn load() -> Res<Self> {
        let work = match optional("WORK").or_else(|| optional("BATTERY_WORK")) {
            Some(path) => PathBuf::from(path),
            None => home()?.join("sbx-battery-work/battery"),
        };
        let evidence = match optional("EVIDENCE") {
            Some(path) => PathBuf::from(path),
            None => work.join("evidence"),
        };
        let timing_lock = match optional("TIMING_LOCK") {
            Some(path) => PathBuf::from(path),
            None => work.join("timing.lock.d"),
        };
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let chart_dir = match optional("CHART_DIR") {
            Some(path) => PathBuf::from(path),
            None => manifest.join("../../../charts/br-sandbox-chart"),
        };
        let values_file = match optional("CHART_VALUES_FILE") {
            Some(path) => PathBuf::from(path),
            None => work.join("values-test.yaml"),
        };
        Ok(Self {
            context: required("CTX_TEST", FILE_CHART_NAMES)?,
            node: required("NODE_TEST", FILE_CHART_NAMES)?,
            ns_prefix: required("NS_PREFIX", FILE_CHART_NAMES)?,
            client: required("CLIENT_A", FILE_CHART_NAMES)?,
            mode: required("NET_NONE", FILE_CHART_NAMES)?,
            rs_prefix: required("RS_PREFIX", FILE_CHART_NAMES)?,
            agent_container: required("SANDBOX_CONTAINER", FILE_CHART_NAMES)?,
            seed_container: required("SEED_CONTAINER", FILE_CHART_NAMES)?,
            seed_root: required("SEED_PATH", FILE_CHART_NAMES)?,
            agent_tools: required("AGENT_BSDTAR", FILE_CHART_NAMES)?,
            seed_tools: required("SEED_BSDTAR", FILE_CHART_NAMES)?,
            python: required("SANDBOX_PYTHON", FILE_CHART_NAMES)?,
            helm_release: required("HELM_RELEASE", FILE_CHART_NAMES)?,
            helm_namespace: required("HELM_NAMESPACE", FILE_CHART_NAMES)?,
            system_ns: required("SYSTEM_NS", FILE_CHART_NAMES)?,
            keeper_deploy: required("KEEPER_DEPLOY", FILE_CHART_NAMES)?,
            runner_ns: required("RUNNER_NS", FILE_CHART_NAMES)?,
            runner_sa: required("RUNNER_SA", FILE_CHART_NAMES)?,
            image_key: required("VALUES_IMAGE_KEY", FILE_CHART_NAMES)?,
            warm_max_age_key: required("WARM_MAX_AGE_VALUE_KEY", FILE_CHART_NAMES)?,
            s3: S3Env {
                endpoint: required("S3_ENDPOINT_HOST", FILE_CREDS)?,
                bucket: required("S3_BUCKET", FILE_CREDS)?,
                region: required("S3_REGION", FILE_CREDS)?,
                path_style: flag("S3_PATH_STYLE", FILE_CREDS)?,
                access_key: required("S3_ACCESS_KEY", FILE_CREDS)?,
                secret_key: required("S3_SECRET_KEY", FILE_CREDS)?,
            },
            evidence,
            work,
            timing_lock,
            chart_dir,
            values_file,
        })
    }

    pub fn namespace(&self) -> String {
        format!("{}-{}-{}", self.ns_prefix, self.client, self.mode)
    }

    pub fn pool_id(&self) -> String {
        format!("{}-{}", self.client, self.mode)
    }

    pub fn replica_set(&self) -> String {
        format!("{}{}", self.rs_prefix, self.pool_id())
    }

    pub fn named(&self, name: &'static str) -> Res<String> {
        required(name, FILE_README)
    }
}
