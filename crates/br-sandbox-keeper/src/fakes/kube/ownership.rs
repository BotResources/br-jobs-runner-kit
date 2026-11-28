use std::collections::BTreeSet;

use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{FieldsV1, ManagedFieldsEntry};
use serde_json::{Value, json};

use crate::cluster::ApiError;

use super::FakeKube;

mod paths;

use paths::{Path, get, merge, paths_of_tree, remove_at, spec_paths, tree_of};

pub(super) const UPDATE: &str = "Update";
pub(super) const APPLY: &str = "Apply";

struct Entry {
    manager: String,
    operation: String,
    paths: BTreeSet<Path>,
}

impl Entry {
    fn is(&self, manager: &str, operation: &str) -> bool {
        self.manager == manager && self.operation == operation
    }
}

fn invalid(name: &str, reason: &str) -> ApiError {
    ApiError::Call(format!(
        "the API server answered 422: ReplicaSet.apps \"{name}\" is invalid: {reason}"
    ))
}

fn entries_of(set: &ReplicaSet) -> Vec<Entry> {
    set.metadata
        .managed_fields
        .iter()
        .flatten()
        .map(|entry| {
            let mut paths = BTreeSet::new();
            if let Some(fields) = &entry.fields_v1 {
                paths_of_tree(&fields.0, &mut Vec::new(), &mut paths);
            }
            Entry {
                manager: entry.manager.clone().unwrap_or_default(),
                operation: entry.operation.clone().unwrap_or_default(),
                paths,
            }
        })
        .collect()
}

fn store(set: &mut ReplicaSet, entries: Vec<Entry>) {
    set.metadata.managed_fields = Some(
        entries
            .into_iter()
            .map(|entry| ManagedFieldsEntry {
                api_version: Some("apps/v1".to_owned()),
                fields_type: Some("FieldsV1".to_owned()),
                fields_v1: Some(FieldsV1(tree_of(&entry.paths))),
                manager: Some(entry.manager),
                operation: Some(entry.operation),
                subresource: None,
                time: None,
            })
            .collect(),
    );
}

fn validated(set: &ReplicaSet) -> Result<(), ApiError> {
    let name = set.metadata.name.clone().unwrap_or_default();
    let Some(pod) = set
        .spec
        .as_ref()
        .and_then(|spec| spec.template.as_ref())
        .and_then(|template| template.spec.as_ref())
    else {
        return Ok(());
    };
    if pod.containers.is_empty() {
        return Err(invalid(
            &name,
            "spec.template.spec.containers: Required value",
        ));
    }
    for (index, container) in pod.containers.iter().enumerate() {
        if container.image.as_deref().is_none_or(str::is_empty) {
            return Err(invalid(
                &name,
                &format!("spec.template.spec.containers[{index}].image: Required value"),
            ));
        }
    }
    Ok(())
}

pub(super) fn record_create(set: &mut ReplicaSet, manager: &str, body: &Value) {
    store(
        set,
        vec![Entry {
            manager: manager.to_owned(),
            operation: UPDATE.to_owned(),
            paths: spec_paths(body),
        }],
    );
}

pub(super) fn apply(set: &mut ReplicaSet, manager: &str, body: &Value) -> Result<(), ApiError> {
    let name = set.metadata.name.clone().unwrap_or_default();
    let mut live =
        serde_json::to_value(&*set).map_err(|error| ApiError::Call(error.to_string()))?;
    let mut entries = entries_of(set);
    let wanted = spec_paths(body);
    for path in &wanted {
        if get(&live, path) != get(body, path) {
            for other in entries.iter_mut().filter(|entry| !entry.is(manager, APPLY)) {
                other.paths.remove(path);
            }
        }
    }
    let index = entries
        .iter()
        .position(|entry| entry.is(manager, APPLY))
        .unwrap_or_else(|| {
            entries.push(Entry {
                manager: manager.to_owned(),
                operation: APPLY.to_owned(),
                paths: BTreeSet::new(),
            });
            entries.len() - 1
        });
    let dropped: Vec<Path> = entries[index].paths.difference(&wanted).cloned().collect();
    entries[index].paths = wanted;
    merge(&mut live, body);
    for path in dropped {
        if !entries.iter().any(|entry| entry.paths.contains(&path)) {
            remove_at(&mut live, &path);
        }
    }
    let mut updated: ReplicaSet = serde_json::from_value(live)
        .map_err(|error| invalid(&name, &format!("the object does not decode: {error}")))?;
    store(&mut updated, entries);
    validated(&updated)?;
    *set = updated;
    Ok(())
}

impl FakeKube {
    pub fn apply_as(
        &self,
        manager: &str,
        namespace: &str,
        name: &str,
        body: &Value,
    ) -> Result<(), ApiError> {
        let mut state = self.state.lock().unwrap();
        let set = state
            .replica_sets
            .get_mut(&(namespace.to_owned(), name.to_owned()))
            .ok_or(ApiError::NotFound)?;
        apply(set, manager, body)
    }

    pub fn rename_field_manager(&self, namespace: &str, name: &str, from: &str, to: &str) {
        let mut state = self.state.lock().unwrap();
        let set = state
            .replica_sets
            .get_mut(&(namespace.to_owned(), name.to_owned()))
            .expect("a known replica set");
        for entry in set.metadata.managed_fields.iter_mut().flatten() {
            if entry.manager.as_deref() == Some(from) {
                entry.manager = Some(to.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests;
