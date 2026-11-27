use std::collections::BTreeMap;

use serde_json::Value;

pub fn merge(target: &mut BTreeMap<String, String>, section: Option<&Value>) {
    let Some(entries) = section.and_then(Value::as_object) else {
        return;
    };
    for (key, value) in entries {
        match value.as_str() {
            Some(text) => target.insert(key.clone(), text.to_owned()),
            None => target.remove(key),
        };
    }
}

pub fn texts(section: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    merge(&mut out, section);
    out
}
