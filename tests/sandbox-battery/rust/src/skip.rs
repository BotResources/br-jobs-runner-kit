use std::path::Path;

use crate::chart_line::ChartLine;
use crate::env::Env;
use crate::error::Res;
use crate::evidence::Checks;

fn reason(chart_version: &str) -> String {
    format!(
        "not for chart {chart_version}: the image is set in the SandboxPool spec; see the README map"
    )
}

pub fn skip_image_test_in(dir: &Path, chart_version: &str, name: &str) -> Res<bool> {
    if ChartLine::of(chart_version)? == ChartLine::Values {
        return Ok(false);
    }
    let mut checks = Checks::in_dir(dir.to_path_buf());
    checks.skip(name, reason(chart_version));
    checks.finish()?;
    Ok(true)
}

pub fn skip_image_test(env: &Env, name: &str) -> Res<bool> {
    skip_image_test_in(&env.evidence, &env.chart_version, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::tests::scratch_dir;

    fn rows(dir: &Path) -> Vec<Vec<String>> {
        std::fs::read_to_string(dir.join("checks.tsv"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.split('\t').map(str::to_owned).collect())
            .collect()
    }

    #[test]
    fn on_the_pool_resource_line_the_test_writes_one_skip_row_with_the_reason_and_no_pass() {
        let dir = scratch_dir("skip-pool-resource");
        assert!(skip_image_test_in(&dir, "0.4.0", "image-case").unwrap());
        let rows = rows(&dir);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], "image-case");
        assert_eq!(rows[0][1], "skip");
        assert_eq!(
            rows[0][2],
            "not for chart 0.4.0: the image is set in the SandboxPool spec; see the README map"
        );
        assert!(rows.iter().all(|row| row[1] != "pass"));
    }

    #[test]
    fn before_0_4_0_the_test_is_not_skipped_and_writes_nothing() {
        let dir = scratch_dir("skip-values");
        assert!(!skip_image_test_in(&dir, "0.3.9", "image-case").unwrap());
        assert!(rows(&dir).is_empty());
    }

    #[test]
    fn a_chart_version_that_is_not_three_numbers_is_an_error_and_writes_nothing() {
        let dir = scratch_dir("skip-bad-version");
        assert!(skip_image_test_in(&dir, "0.4", "image-case").is_err());
        assert!(rows(&dir).is_empty());
    }
}
