use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::admissionregistration::v1::{
    MatchResources, ValidatingAdmissionPolicyBinding, ValidatingAdmissionPolicyBindingSpec,
};
use k8s_openapi::api::apps::v1::{ReplicaSet, ReplicaSetSpec};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, ResourceQuota};
use k8s_openapi::api::networking::v1::{NetworkPolicy, NetworkPolicySpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::fakes::FakeKube;
use crate::pool::{
    ADMISSION_BINDING_NAME, Owner, POD_TEMPLATE_KEY, QUOTA_NAME, owner_reference, replicaset_name,
};
use crate::resource::SandboxPool;
use crate::rights::{NETWORK_POLICY_DEFAULT_DENY, POD_TEMPLATE_CONFIGMAP};

use super::{PoolApi, Views};

pub const SELECTOR: &str = "br.sandbox/managed=true";
pub const AGENT: &str = "agent";

pub fn named<T: Default>(name: &str, build: impl FnOnce(ObjectMeta) -> T) -> T {
    build(ObjectMeta {
        name: Some(name.to_owned()),
        ..ObjectMeta::default()
    })
}

pub fn namespace(name: &str, labels: &[(&str, &str)]) -> Namespace {
    Namespace {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            labels: Some(
                labels
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            ),
            ..ObjectMeta::default()
        },
        ..Namespace::default()
    }
}

pub fn managed(name: &str) -> Namespace {
    namespace(name, &[("br.sandbox/managed", "true")])
}

pub fn binding() -> ValidatingAdmissionPolicyBinding {
    named(ADMISSION_BINDING_NAME, |metadata| {
        ValidatingAdmissionPolicyBinding {
            metadata,
            spec: Some(ValidatingAdmissionPolicyBindingSpec {
                match_resources: Some(MatchResources {
                    namespace_selector: Some(LabelSelector {
                        match_labels: Some(BTreeMap::from([(
                            "br.sandbox/managed".to_owned(),
                            "true".to_owned(),
                        )])),
                        match_expressions: None,
                    }),
                    ..MatchResources::default()
                }),
                ..ValidatingAdmissionPolicyBindingSpec::default()
            }),
        }
    })
}

pub fn quota() -> ResourceQuota {
    named(QUOTA_NAME, |metadata| ResourceQuota {
        metadata,
        ..ResourceQuota::default()
    })
}

pub fn deny_all() -> NetworkPolicy {
    named(NETWORK_POLICY_DEFAULT_DENY, |metadata| NetworkPolicy {
        metadata,
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(LabelSelector::default()),
            policy_types: Some(vec!["Ingress".to_owned(), "Egress".to_owned()]),
            ..NetworkPolicySpec::default()
        }),
    })
}

pub fn template_json(namespace: &str) -> Value {
    json!({
        "metadata": {"labels": {
            "br.sandbox/pool": "warm",
            "br.sandbox/pool-id": namespace,
        }},
        "spec": {"containers": [{"name": AGENT, "image": "placeholder"}]}
    })
}

pub fn template_map(template: &Value) -> ConfigMap {
    ConfigMap {
        metadata: ObjectMeta {
            name: Some(POD_TEMPLATE_CONFIGMAP.to_owned()),
            ..ObjectMeta::default()
        },
        data: Some(BTreeMap::from([(
            POD_TEMPLATE_KEY.to_owned(),
            template.to_string(),
        )])),
        ..ConfigMap::default()
    }
}

pub fn pool(name: &str, uid: &str) -> SandboxPool {
    SandboxPool {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            uid: Some(uid.to_owned()),
            ..ObjectMeta::default()
        },
        spec: Value::Null,
        status: None,
    }
}

pub fn owner() -> Owner {
    Owner::new("main", "uid-1")
}

pub fn owned_replica_set(namespace: &str, replicas: i32) -> ReplicaSet {
    ReplicaSet {
        metadata: ObjectMeta {
            name: Some(replicaset_name(namespace)),
            namespace: Some(namespace.to_owned()),
            uid: Some("rs-uid".to_owned()),
            owner_references: Some(vec![owner_reference(&owner())]),
            ..ObjectMeta::default()
        },
        spec: Some(ReplicaSetSpec {
            replicas: Some(replicas),
            ..ReplicaSetSpec::default()
        }),
        status: None,
    }
}

pub struct Bed {
    pub kube: Arc<FakeKube>,
    pub views: Views,
    pub stop: CancellationToken,
}

impl Bed {
    pub fn start() -> Self {
        let kube = Arc::new(FakeKube::default());
        let views = Views::new();
        let stop = CancellationToken::new();
        let api: Arc<dyn PoolApi> = kube.clone();
        tokio::spawn(views.clone().run(api, SELECTOR.to_owned(), stop.clone()));
        Self { kube, views, stop }
    }

    pub async fn until(&self, what: &str, ready: impl Fn(&Views) -> bool) {
        let mut changes = self.views.subscribe();
        let waiting = async {
            loop {
                if ready(&self.views) {
                    return;
                }
                changes.changed().await.expect("the views are alive");
            }
        };
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap_or_else(|_| panic!("the views never reached: {what}"));
    }
}

impl Drop for Bed {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
