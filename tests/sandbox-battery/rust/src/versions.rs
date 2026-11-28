use std::path::Path;

use serde_json::Value;

use crate::env::Env;
use crate::error::{BatteryError, Res};
use crate::proc::Kube;
use crate::release_chart::ReleaseChart;

const MANIFEST: &str = include_str!("../Cargo.toml");

fn quoted_after(line: &str, key: &str) -> Option<String> {
    let rest = line.split_once(key)?.1.trim_start().strip_prefix('"')?;
    Some(rest.split_once('"')?.0.to_owned())
}

pub fn adapter_version_in(manifest: &str) -> Option<String> {
    manifest
        .lines()
        .find(|line| line.starts_with("br-runner-sandbox = "))
        .and_then(|line| quoted_after(line, "version = "))
}

pub fn chart_version_in(chart: &str) -> Option<String> {
    chart
        .lines()
        .find(|line| line.starts_with("version:"))
        .map(|line| {
            line.trim_start_matches("version:")
                .trim()
                .trim_matches('"')
                .to_owned()
        })
        .filter(|version| !version.is_empty())
}

pub fn chart_version(chart_dir: &Path) -> Res<String> {
    let file = chart_dir.join("Chart.yaml");
    let text = std::fs::read_to_string(&file)?;
    chart_version_in(&text).ok_or_else(|| BatteryError::Data {
        what: file.display().to_string(),
        reason: "no version line".to_owned(),
    })
}

pub fn container_image_in(deployment: &Value, container: &str) -> Option<String> {
    deployment["spec"]["template"]["spec"]["containers"]
        .as_array()?
        .iter()
        .find(|candidate| candidate["name"].as_str() == Some(container))?["image"]
        .as_str()
        .map(str::to_owned)
}

pub async fn keeper_image(env: &Env) -> Res<String> {
    let deployment = Kube::cluster_wide(env)
        .in_namespace(&env.system_ns)
        .json(&["get", "deployment", &env.keeper_deploy])
        .await?;
    container_image_in(&deployment, &env.keeper_container).ok_or_else(|| BatteryError::Data {
        what: format!("deployment {}", env.keeper_deploy),
        reason: format!("no container named {} with an image", env.keeper_container),
    })
}

pub async fn line(env: &Env) -> Res<String> {
    let adapter = adapter_version_in(MANIFEST).ok_or_else(|| BatteryError::Data {
        what: "Cargo.toml".to_owned(),
        reason: "no br-runner-sandbox version".to_owned(),
    })?;
    Ok(format!(
        "adapter {adapter}, chart {}, keeper {}",
        ReleaseChart::of(env).measured_version()?,
        keeper_image(env).await?
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_adapter_version_is_the_one_pinned_in_the_manifest() {
        let manifest = "[dependencies]\nbr-runner-sandbox = { git = \"https://example.test/x\", package = \"br-runner-sandbox\", tag = \"br-runner-sandbox/v0.1.3\", version = \"0.1.3\" }\nbytes = \"1\"\n";
        assert_eq!(adapter_version_in(manifest).as_deref(), Some("0.1.3"));
        assert_eq!(adapter_version_in("bytes = \"1\"\n"), None);
        assert!(adapter_version_in(MANIFEST).is_some());
    }

    #[test]
    fn the_chart_version_is_the_version_line_and_not_the_app_version() {
        let chart = "apiVersion: v2\nname: x\nversion: 0.4.0\nappVersion: \"0.9.9\"\n";
        assert_eq!(chart_version_in(chart).as_deref(), Some("0.4.0"));
        assert_eq!(
            chart_version_in("version: \"1.2.3\"\n").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(chart_version_in("name: x\n"), None);
        assert_eq!(chart_version_in("version:\n"), None);
    }

    #[test]
    fn the_keeper_image_is_read_from_the_container_named_as_the_keeper() {
        let deployment = json!({"spec": {"template": {"spec": {"containers": [
            {"name": "sidecar", "image": "example.test/sidecar:1"},
            {"name": "keeper", "image": "example.test/keeper@sha256:abc"}
        ]}}}});
        assert_eq!(
            container_image_in(&deployment, "keeper").as_deref(),
            Some("example.test/keeper@sha256:abc")
        );
        assert_eq!(container_image_in(&deployment, "other"), None);
        assert_eq!(container_image_in(&json!({}), "keeper"), None);
    }

    #[test]
    fn the_chart_of_this_repository_has_a_version() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../charts/br-sandbox-chart");
        assert!(chart_version(&dir).is_ok());
    }
}
