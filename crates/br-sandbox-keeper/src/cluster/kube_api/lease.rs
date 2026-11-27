use std::time::Duration;

use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta, Time};

use crate::convert::timestamp_of;

use super::{LeaseFacts, LeaseWrite, instant};

pub(super) fn lease_facts(lease: &Lease) -> LeaseFacts {
    let spec = lease.spec.clone().unwrap_or_default();
    LeaseFacts {
        holder: spec.holder_identity.filter(|holder| !holder.is_empty()),
        acquire_time: spec
            .acquire_time
            .as_ref()
            .map(|time| timestamp_of(&Time(time.0))),
        renew_time: spec
            .renew_time
            .as_ref()
            .map(|time| timestamp_of(&Time(time.0))),
        duration: spec
            .lease_duration_seconds
            .and_then(|seconds| u64::try_from(seconds).ok())
            .map(Duration::from_secs),
        transitions: spec.lease_transitions.unwrap_or(0),
        resource_version: lease.metadata.resource_version.clone().unwrap_or_default(),
    }
}

pub(super) fn lease_object(
    name: &str,
    namespace: &str,
    write: &LeaseWrite,
    version: Option<&str>,
) -> Lease {
    Lease {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            resource_version: version.map(str::to_owned),
            ..ObjectMeta::default()
        },
        spec: Some(LeaseSpec {
            holder_identity: Some(write.holder.clone()),
            acquire_time: Some(MicroTime(instant(write.acquire_time))),
            renew_time: Some(MicroTime(instant(write.renew_time))),
            lease_duration_seconds: i32::try_from(write.duration.as_secs()).ok(),
            lease_transitions: Some(write.transitions),
            ..LeaseSpec::default()
        }),
    }
}
