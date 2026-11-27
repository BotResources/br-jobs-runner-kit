use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the label selector is not valid: {0}")]
pub struct SelectorError(String);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Requirement {
    Equals(String, String),
    NotEquals(String, String),
    In(String, Vec<String>),
    NotIn(String, Vec<String>),
    Exists(String),
    Absent(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector(Vec<Requirement>);

fn refuse<T>(reason: impl Into<String>) -> Result<T, SelectorError> {
    Err(SelectorError(reason.into()))
}

fn split_requirements(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for character in text.chars() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    parts.push(current);
    parts
}

fn name_ok(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

fn value_ok(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn name(text: &str) -> Result<String, SelectorError> {
    if name_ok(text) {
        Ok(text.to_owned())
    } else {
        refuse(format!("{text:?} is not a label key"))
    }
}

fn value(text: &str) -> Result<String, SelectorError> {
    if value_ok(text) {
        Ok(text.to_owned())
    } else {
        refuse(format!("{text:?} is not a label value"))
    }
}

fn values_of(list: &str) -> Result<Vec<String>, SelectorError> {
    let inner = list
        .trim()
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(inner) = inner else {
        return refuse(format!("{list:?} is not a list of values"));
    };
    inner.split(',').map(|item| value(item.trim())).collect()
}

fn set_requirement(text: &str) -> Result<Option<Requirement>, SelectorError> {
    for (word, negated) in [(" notin ", true), (" in ", false)] {
        if let Some((key, list)) = text.split_once(word) {
            let key = name(key.trim())?;
            let values = values_of(list)?;
            return Ok(Some(if negated {
                Requirement::NotIn(key, values)
            } else {
                Requirement::In(key, values)
            }));
        }
    }
    Ok(None)
}

fn requirement(text: &str) -> Result<Requirement, SelectorError> {
    let text = text.trim();
    if text.is_empty() {
        return refuse("a requirement is empty");
    }
    if let Some(found) = set_requirement(text)? {
        return Ok(found);
    }
    if let Some((key, wanted)) = text.split_once("!=") {
        return Ok(Requirement::NotEquals(
            name(key.trim())?,
            value(wanted.trim())?,
        ));
    }
    if let Some((key, wanted)) = text.split_once("==").or_else(|| text.split_once('=')) {
        return Ok(Requirement::Equals(
            name(key.trim())?,
            value(wanted.trim())?,
        ));
    }
    match text.strip_prefix('!') {
        Some(key) => Ok(Requirement::Absent(name(key.trim())?)),
        None => Ok(Requirement::Exists(name(text)?)),
    }
}

impl Selector {
    pub fn parse(text: &str) -> Result<Self, SelectorError> {
        if text.trim().is_empty() {
            return Ok(Self(Vec::new()));
        }
        split_requirements(text)
            .iter()
            .map(|part| requirement(part))
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    pub fn from_label_selector(selector: &LabelSelector) -> Result<Self, SelectorError> {
        let mut requirements: Vec<Requirement> = selector
            .match_labels
            .iter()
            .flatten()
            .map(|(key, wanted)| Requirement::Equals(key.clone(), wanted.clone()))
            .collect();
        for expression in selector.match_expressions.iter().flatten() {
            let key = expression.key.clone();
            let values = expression.values.clone().unwrap_or_default();
            requirements.push(match expression.operator.as_str() {
                "In" => Requirement::In(key, values),
                "NotIn" => Requirement::NotIn(key, values),
                "Exists" => Requirement::Exists(key),
                "DoesNotExist" => Requirement::Absent(key),
                other => return refuse(format!("the operator {other:?} is not known")),
            });
        }
        Ok(Self(requirements))
    }

    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.0.iter().all(|requirement| match requirement {
            Requirement::Equals(key, wanted) => labels.get(key) == Some(wanted),
            Requirement::NotEquals(key, wanted) => labels.get(key) != Some(wanted),
            Requirement::In(key, values) => labels.get(key).is_some_and(|got| values.contains(got)),
            Requirement::NotIn(key, values) => {
                !labels.get(key).is_some_and(|got| values.contains(got))
            }
            Requirement::Exists(key) => labels.contains_key(key),
            Requirement::Absent(key) => !labels.contains_key(key),
        })
    }
}

#[cfg(test)]
mod tests;
