use crate::error::{BatteryError, Res};

const FIRST_NAMESPACE_POOL_ID: (u64, u64, u64) = (0, 4, 0);

fn numbers(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let triple = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(triple)
}

fn bad(name: &'static str, value: &str, expected: &'static str) -> BatteryError {
    BatteryError::BadVariable {
        name,
        value: value.to_owned(),
        expected,
    }
}

fn strip_of(chart_version: &str, ns_prefix: &str, replace: Option<&str>) -> Res<String> {
    if let Some(prefix) = replace {
        return Ok(prefix.to_owned());
    }
    let version = numbers(chart_version).ok_or_else(|| {
        bad(
            "CHART_VERSION",
            chart_version,
            "a version such as 0.4.0 (three numbers)",
        )
    })?;
    if version >= FIRST_NAMESPACE_POOL_ID {
        Ok(String::new())
    } else {
        Ok(format!("{ns_prefix}-"))
    }
}

pub fn pool_id(
    namespace: &str,
    ns_prefix: &str,
    chart_version: &str,
    replace: Option<&str>,
) -> Res<String> {
    let strip = strip_of(chart_version, ns_prefix, replace)?;
    match namespace.strip_prefix(strip.as_str()) {
        Some(id) if !id.is_empty() => Ok(id.to_owned()),
        _ => Err(bad(
            "POOL_ID_STRIP",
            &strip,
            "a prefix of the namespace name that leaves a pool id",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(version: &str, replace: Option<&str>) -> Res<String> {
        pool_id("sbx-ca-none", "sbx", version, replace)
    }

    #[test]
    fn below_0_4_0_the_pool_id_is_the_namespace_without_the_prefix() {
        assert_eq!(id("0.2.4", None).unwrap(), "ca-none");
        assert_eq!(id("0.3.9", None).unwrap(), "ca-none");
    }

    #[test]
    fn from_0_4_0_on_the_pool_id_is_the_namespace() {
        for version in [
            "0.4.0",
            "0.4.1",
            "0.10.0",
            "1.0.0",
            "0.4.0-rc1",
            "0.4.0+build",
        ] {
            assert_eq!(id(version, None).unwrap(), "sbx-ca-none", "{version}");
        }
    }

    #[test]
    fn a_replacement_prefix_wins_over_the_chart_version_also_when_empty() {
        assert_eq!(id("0.4.0", Some("sbx-")).unwrap(), "ca-none");
        assert_eq!(id("0.2.4", Some("")).unwrap(), "sbx-ca-none");
    }

    #[test]
    fn a_chart_version_that_is_not_three_numbers_fails() {
        for version in ["", "0.4", "x.y.z", "0.4.0.1", "v0.4.0"] {
            let error = id(version, None).unwrap_err().to_string();
            assert!(error.contains("CHART_VERSION"), "{version}: {error}");
        }
    }

    #[test]
    fn a_prefix_that_does_not_fit_the_namespace_fails() {
        for strip in ["other-", "sbx-ca-none"] {
            let error = id("0.4.0", Some(strip)).unwrap_err().to_string();
            assert!(error.contains("POOL_ID_STRIP"), "{strip}: {error}");
        }
    }
}
