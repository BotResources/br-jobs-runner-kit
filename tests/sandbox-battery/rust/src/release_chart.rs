use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::error::{Res, data};
use crate::versions::chart_version_in;

#[derive(Debug, Clone)]
pub struct ReleaseChart {
    given: bool,
    dir: PathBuf,
    version: String,
}

fn same_version(dir: &Path, chart_yaml: Option<&str>, release: &str) -> Res<String> {
    let found = chart_yaml.and_then(chart_version_in).ok_or_else(|| {
        data(
            "CHART_DIR",
            format!(
                "{} holds no readable Chart.yaml with a version",
                dir.display()
            ),
        )
    })?;
    if found == release {
        return Ok(found);
    }
    Err(data(
        "CHART_DIR",
        format!(
            "{} is chart {found} but the installed release is {release} (CHART_VERSION)",
            dir.display()
        ),
    ))
}

fn verdict(given: bool, dir: &Path, chart_yaml: Option<&str>, release: &str) -> Res {
    if !given {
        return Err(data(
            "CHART_DIR",
            format!(
                "not set: a test that changes the chart values needs the folder of the chart of the installed release ({release})"
            ),
        ));
    }
    same_version(dir, chart_yaml, release).map(|_| ())
}

impl ReleaseChart {
    pub fn of(env: &Env) -> Self {
        Self {
            given: env.chart_dir_given,
            dir: env.chart_dir.clone(),
            version: env.chart_version.clone(),
        }
    }

    fn chart_yaml(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join("Chart.yaml")).ok()
    }

    pub fn check(&self) -> Res {
        verdict(
            self.given,
            &self.dir,
            self.chart_yaml().as_deref(),
            &self.version,
        )
    }

    pub fn measured_version(&self) -> Res<String> {
        same_version(&self.dir, self.chart_yaml().as_deref(), &self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART: &str = "apiVersion: v2\nname: x\nversion: 0.4.0\n";

    fn run(given: bool, yaml: Option<&str>, release: &str) -> Res {
        verdict(given, Path::new("/chart"), yaml, release)
    }

    #[test]
    fn the_chart_of_the_release_is_accepted() {
        assert!(run(true, Some(CHART), "0.4.0").is_ok());
    }

    #[test]
    fn an_unset_chart_dir_fails_and_names_the_variable() {
        let error = run(false, Some(CHART), "0.4.0").unwrap_err().to_string();
        assert!(
            error.contains("CHART_DIR") && error.contains("not set"),
            "{error}"
        );
    }

    #[test]
    fn another_chart_version_fails_and_names_both_versions() {
        let error = run(true, Some(CHART), "0.2.4").unwrap_err().to_string();
        assert!(
            error.contains("0.4.0") && error.contains("0.2.4"),
            "{error}"
        );
    }

    #[test]
    fn a_folder_without_a_chart_fails() {
        let error = run(true, None, "0.4.0").unwrap_err().to_string();
        assert!(error.contains("no readable Chart.yaml"), "{error}");
        assert!(run(true, Some("name: x\n"), "0.4.0").is_err());
    }

    fn chart_folder(name: &str, yaml: &str, release: &str, given: bool) -> ReleaseChart {
        let dir = std::env::temp_dir().join(format!(
            "battery-release-chart-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Chart.yaml"), yaml).unwrap();
        ReleaseChart {
            given,
            dir,
            version: release.to_owned(),
        }
    }

    #[test]
    fn a_measured_version_is_the_one_the_chart_folder_holds() {
        let chart = chart_folder("same", CHART, "0.4.0", false);
        assert_eq!(chart.measured_version().unwrap(), "0.4.0");
    }

    #[test]
    fn a_measured_version_fails_when_the_folder_holds_another_chart_even_if_no_folder_was_given() {
        let chart = chart_folder("other", "version: 0.3.0\n", "0.4.0", false);
        let error = chart.measured_version().unwrap_err().to_string();
        assert!(
            error.contains("0.3.0") && error.contains("0.4.0") && error.contains("CHART_VERSION"),
            "{error}"
        );
    }

    #[test]
    fn a_measured_version_fails_when_the_folder_has_no_chart() {
        let chart = ReleaseChart {
            given: true,
            dir: std::env::temp_dir()
                .join(format!("battery-release-chart-none-{}", std::process::id())),
            version: "0.4.0".to_owned(),
        };
        let error = chart.measured_version().unwrap_err().to_string();
        assert!(error.contains("no readable Chart.yaml"), "{error}");
    }
}
