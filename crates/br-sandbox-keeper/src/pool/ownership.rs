use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

use super::names::replicaset_name;
use crate::resource::{GROUP, KIND, SandboxPool, VERSION};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Owner {
    pub name: String,
    pub uid: String,
}

impl Owner {
    pub fn new(name: impl Into<String>, uid: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            uid: uid.into(),
        }
    }

    pub fn of(pool: &SandboxPool) -> Option<Self> {
        let name = pool
            .metadata
            .name
            .as_deref()
            .filter(|name| !name.is_empty())?;
        let uid = pool.metadata.uid.as_deref().filter(|uid| !uid.is_empty())?;
        Some(Self::new(name, uid))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OwnershipFault {
    #[error("the ReplicaSet is not in the namespace of the resource")]
    OtherNamespace,
    #[error("the ReplicaSet does not have the name of the pool")]
    OtherName,
    #[error("the ReplicaSet is being deleted")]
    Deleting,
    #[error("no owner reference of the ReplicaSet is the controller reference of this resource")]
    NotControlledByResource,
}

fn group_of(api_version: &str) -> &str {
    api_version
        .split_once('/')
        .map_or("", |(group, _version)| group)
}

fn controls(reference: &OwnerReference, owner: &Owner) -> bool {
    reference.controller == Some(true)
        && group_of(&reference.api_version) == GROUP
        && reference.kind == KIND
        && reference.uid == owner.uid
}

pub fn check_ownership(
    replica_set: &ReplicaSet,
    namespace: &str,
    owner: &Owner,
) -> Result<(), OwnershipFault> {
    let meta = &replica_set.metadata;
    if meta.namespace.as_deref() != Some(namespace) {
        return Err(OwnershipFault::OtherNamespace);
    }
    if meta.name.as_deref() != Some(replicaset_name(namespace).as_str()) {
        return Err(OwnershipFault::OtherName);
    }
    if meta.deletion_timestamp.is_some() {
        return Err(OwnershipFault::Deleting);
    }
    let controlled = meta
        .owner_references
        .iter()
        .flatten()
        .any(|reference| controls(reference, owner));
    if controlled {
        Ok(())
    } else {
        Err(OwnershipFault::NotControlledByResource)
    }
}

pub fn owner_reference(owner: &Owner) -> OwnerReference {
    OwnerReference {
        api_version: format!("{GROUP}/{VERSION}"),
        kind: KIND.to_owned(),
        name: owner.name.clone(),
        uid: owner.uid.clone(),
        controller: Some(true),
        block_owner_deletion: None,
    }
}

#[cfg(test)]
mod tests;
