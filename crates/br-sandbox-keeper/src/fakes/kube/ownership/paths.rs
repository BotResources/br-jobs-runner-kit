use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

pub(super) type Path = Vec<String>;

pub(super) fn item_name(item: &Value) -> Option<&str> {
    item.as_object()?.get("name")?.as_str()
}

pub(super) fn is_keyed(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|item| item_name(item).is_some())
}

pub(super) fn item_segment(name: &str) -> String {
    format!("k:{}", json!({ "name": name }))
}

pub(super) fn segment_name(segment: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(segment.strip_prefix("k:")?).ok()?;
    parsed.get("name")?.as_str().map(str::to_owned)
}

pub(super) fn collect(value: &Value, prefix: &mut Path, found: &mut BTreeSet<Path>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                prefix.push(format!("f:{key}"));
                collect(child, prefix, found);
                prefix.pop();
            }
        }
        Value::Array(items) if is_keyed(items) => {
            for item in items {
                prefix.push(item_segment(item_name(item).unwrap_or_default()));
                collect(item, prefix, found);
                prefix.pop();
            }
        }
        _ => {
            found.insert(prefix.clone());
        }
    }
}

pub(super) fn spec_paths(body: &Value) -> BTreeSet<Path> {
    let mut found = BTreeSet::new();
    if let Some(spec) = body.get("spec") {
        collect(spec, &mut vec!["f:spec".to_owned()], &mut found);
    }
    found
}

pub(super) fn tree_of(paths: &BTreeSet<Path>) -> Value {
    let mut root = Value::Object(Map::new());
    for path in paths {
        let mut node = &mut root;
        for segment in path {
            node = node
                .as_object_mut()
                .expect("a tree node is an object")
                .entry(segment.clone())
                .or_insert_with(|| Value::Object(Map::new()));
        }
    }
    root
}

pub(super) fn paths_of_tree(tree: &Value, prefix: &mut Path, found: &mut BTreeSet<Path>) {
    let children: Vec<_> = tree
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.as_str() != ".")
        .collect();
    if children.is_empty() {
        found.insert(prefix.clone());
        return;
    }
    for (key, child) in children {
        prefix.push(key.clone());
        paths_of_tree(child, prefix, found);
        prefix.pop();
    }
}

pub(super) fn step<'a>(value: &'a Value, segment: &str) -> Option<&'a Value> {
    match (segment.strip_prefix("f:"), segment_name(segment)) {
        (Some(key), _) => value.get(key),
        (None, Some(name)) => value
            .as_array()?
            .iter()
            .find(|item| item_name(item) == Some(name.as_str())),
        (None, None) => None,
    }
}

pub(super) fn get<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |node, segment| step(node, segment))
}

pub(super) fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty)
}

pub(super) fn remove_at(value: &mut Value, path: &[String]) {
    let Some((head, rest)) = path.split_first() else {
        return;
    };
    if let Some(key) = head.strip_prefix("f:") {
        let Some(map) = value.as_object_mut() else {
            return;
        };
        if rest.is_empty() {
            map.remove(key);
        } else if let Some(child) = map.get_mut(key) {
            remove_at(child, rest);
            if is_empty_object(child) {
                map.remove(key);
            }
        }
    } else if let Some(name) = segment_name(head) {
        let Some(items) = value.as_array_mut() else {
            return;
        };
        let Some(index) = items
            .iter()
            .position(|item| item_name(item) == Some(name.as_str()))
        else {
            return;
        };
        if rest.is_empty() {
            items.remove(index);
        } else {
            remove_at(&mut items[index], rest);
            if is_empty_object(&items[index]) {
                items.remove(index);
            }
        }
    }
}

pub(super) fn merge(live: &mut Value, body: &Value) {
    match (live, body) {
        (Value::Object(live), Value::Object(body)) => {
            for (key, child) in body {
                match live.get_mut(key) {
                    Some(held) => merge(held, child),
                    None => {
                        live.insert(key.clone(), child.clone());
                    }
                }
            }
        }
        (Value::Array(live), Value::Array(body)) if is_keyed(body) => {
            for item in body {
                match live
                    .iter_mut()
                    .find(|held| item_name(held) == item_name(item))
                {
                    Some(held) => merge(held, item),
                    None => live.push(item.clone()),
                }
            }
        }
        (live, body) => *live = body.clone(),
    }
}
