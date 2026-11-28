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

fn verdict(given: bool, dir: &Path, chart_yaml: Option<&str>, release: &str) -> Res {
    if !given {
        return Err(data(
            "CHART_DIR",
            format!(
                "not set: a test that changes the chart values needs the folder of the chart of the installed release ({release})"
            ),
        ));
    }
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
        return Ok(());
    }
    Err(data(
        "CHART_DIR",
        format!(
            "{} is chart {found} but the installed release is {release} (CHART_VERSION): a restore would put the wrong chart back",
            dir.display()
        ),
    ))
}

impl ReleaseChart {
    pub fn of(env: &Env) -> Self {
        Self {
            given: env.chart_dir_given,
            dir: env.chart_dir.clone(),
            version: env.chart_version.clone(),
        }
    }

    pub fn check(&self) -> Res {
        let text = std::fs::read_to_string(self.dir.join("Chart.yaml")).ok();
        verdict(self.given, &self.dir, text.as_deref(), &self.version)
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
}
