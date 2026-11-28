use crate::error::{BatteryError, Res};

const FIRST_POOL_RESOURCE: (u64, u64, u64) = (0, 4, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartLine {
    Values,
    PoolResource,
}

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

impl ChartLine {
    pub fn of(chart_version: &str) -> Res<Self> {
        let version = numbers(chart_version).ok_or_else(|| BatteryError::BadVariable {
            name: "CHART_VERSION",
            value: chart_version.to_owned(),
            expected: "a version such as 0.4.0 (three numbers)",
        })?;
        if version >= FIRST_POOL_RESOURCE {
            Ok(Self::PoolResource)
        } else {
            Ok(Self::Values)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_0_4_0_the_pool_is_set_by_chart_values() {
        for version in ["0.2.4", "0.3.9", "0.3.99+build"] {
            assert_eq!(
                ChartLine::of(version).unwrap(),
                ChartLine::Values,
                "{version}"
            );
        }
    }

    #[test]
    fn from_0_4_0_on_the_pool_is_a_resource() {
        for version in [
            "0.4.0",
            "0.4.1",
            "0.10.0",
            "1.0.0",
            "0.4.0-rc1",
            "0.4.0+build",
        ] {
            assert_eq!(
                ChartLine::of(version).unwrap(),
                ChartLine::PoolResource,
                "{version}"
            );
        }
    }

    #[test]
    fn a_chart_version_that_is_not_three_numbers_fails() {
        for version in ["", "0.4", "x.y.z", "0.4.0.1", "v0.4.0"] {
            let error = ChartLine::of(version).unwrap_err().to_string();
            assert!(error.contains("CHART_VERSION"), "{version}: {error}");
        }
    }
}
