use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry;

use super::TEMPLATE_FIELD_MANAGER;

const APPLY: &str = "Apply";

fn holds_template(entry: &ManagedFieldsEntry, manager: &str) -> bool {
    entry.manager.as_deref() == Some(manager)
        && entry.operation.as_deref() == Some(APPLY)
        && entry.subresource.is_none()
        && entry
            .fields_v1
            .as_ref()
            .and_then(|fields| fields.0.get("f:spec"))
            .and_then(|spec| spec.get("f:template"))
            .is_some()
}

fn template_held_by(replica_set: &ReplicaSet, manager: &str) -> bool {
    replica_set
        .metadata
        .managed_fields
        .iter()
        .flatten()
        .any(|entry| holds_template(entry, manager))
}

fn managed_fields_are_readable(replica_set: &ReplicaSet) -> bool {
    replica_set
        .metadata
        .managed_fields
        .as_ref()
        .is_some_and(|entries| !entries.is_empty())
}

fn template_hash_of<'a>(replica_set: &'a ReplicaSet, key: &str) -> Option<&'a str> {
    replica_set
        .spec
        .as_ref()?
        .template
        .as_ref()?
        .metadata
        .as_ref()?
        .annotations
        .as_ref()?
        .get(key)
        .map(String::as_str)
}

pub(crate) fn template_in_place(replica_set: &ReplicaSet, key: &str, hash: &str) -> bool {
    template_hash_of(replica_set, key) == Some(hash)
        && (template_held_by(replica_set, TEMPLATE_FIELD_MANAGER)
            || !managed_fields_are_readable(replica_set))
}

#[cfg(test)]
mod tests;
