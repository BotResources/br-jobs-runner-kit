use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LabelSelector {
    pairs: BTreeMap<String, String>,
}

impl LabelSelector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.pairs.insert(key.into(), value.into());
        self
    }

    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.pairs
            .iter()
            .all(|(key, value)| labels.get(key) == Some(value))
    }

    pub fn pairs(&self) -> &BTreeMap<String, String> {
        &self.pairs
    }
}

impl fmt::Display for LabelSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (key, value) in &self.pairs {
            if !first {
                f.write_str(",")?;
            }
            first = false;
            write!(f, "{key}={value}")?;
        }
        Ok(())
    }
}
