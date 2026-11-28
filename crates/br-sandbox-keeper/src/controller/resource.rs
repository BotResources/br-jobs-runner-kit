use k8s_openapi::api::apps::v1::ReplicaSet;

use crate::cluster::{
    APPLY_TEMPLATE, CREATE_REPLICA_SET, DELETE_REPLICA_SET, DeleteOutcome, ReplicaSetRead,
    template_in_place,
};
use crate::pool::{
    Acceptance, FenceVerdict, Owner, PoolSettings, PoolTemplate, RenderedTemplate, TemplateFault,
    judge_acceptance, template_hash_key, validate,
};
use crate::resource::SandboxPool;
use crate::rules::Desired;

use super::Engine;
use super::condition::target_of;
use super::observation::replicas_of;

type Template = Result<PoolTemplate, TemplateFault>;

struct Served {
    held: Option<ReplicaSetRead>,
    hash: Option<String>,
}

impl Engine {
    pub(super) async fn reconcile_pool(
        &mut self,
        namespace: &str,
        pool: &SandboxPool,
        fence: Option<&FenceVerdict>,
        duplicate: bool,
        pods_synced: bool,
    ) {
        let Some(owner) = Owner::of(pool) else {
            return;
        };
        let settings = validate(pool);
        let fenced = matches!(fence, Some(FenceVerdict::Fenced));
        let template = if fenced && !duplicate && settings.is_ok() {
            self.read_template(namespace).await
        } else {
            None
        };
        let acceptance = judge_acceptance(fence, Some(duplicate), &settings, template.as_ref());
        let minimum = settings.as_ref().ok().map(|found| found.min_replicas);
        if duplicate {
            self.write_status(namespace, pool, &acceptance, None, minimum)
                .await;
            return;
        }
        let held = if pods_synced && fence.is_some() {
            self.observer
                .replica_set(namespace, &owner)
                .await
                .observed()
        } else {
            None
        };
        let sized = settings
            .as_ref()
            .ok()
            .map(|found| (found, self.size(namespace, found)));
        let desired = sized.as_ref().map(|(_, size)| size.clamped);
        let served = match (&acceptance, sized, &template) {
            (Acceptance::Valid, Some((found, size)), Some(Ok(template))) => {
                self.serve(namespace, &owner, (found, template), size, held)
                    .await
            }
            (Acceptance::NamespaceNotFenced(_), _, _) => Served {
                held: self.remove_replica_set(namespace, &owner, held).await,
                hash: None,
            },
            _ => Served { held, hash: None },
        };
        let observation = self.observation(
            namespace,
            pods_synced,
            served.held.as_ref(),
            desired,
            served.hash.as_deref(),
        );
        if let (Some(ReplicaSetRead::Owned(replica_set)), Some(observed)) =
            (&served.held, &observation)
        {
            self.mirror_condition(namespace, &owner, replica_set, &observed.failing)
                .await;
            let target = target_of(namespace, replica_set);
            self.pending_events(namespace, &target).await;
        }
        let observation =
            observation.map(|observed| observed.refused(self.refusals.naming(namespace)));
        self.write_status(namespace, pool, &acceptance, observation.as_ref(), minimum)
            .await;
    }

    async fn read_template(&mut self, namespace: &str) -> Option<Template> {
        let read = self
            .observer
            .template(
                namespace,
                &self.config.pod_keys(),
                &self.config.agent_container,
            )
            .await;
        match read.observed() {
            Some(template) => Some(template),
            None => {
                self.action_failed(
                    "the ConfigMap of the pod template could not be read",
                    namespace,
                    &"not observed",
                );
                self.retry_soon(namespace);
                None
            }
        }
    }

    async fn serve(
        &mut self,
        namespace: &str,
        owner: &Owner,
        (settings, template): (&PoolSettings, &PoolTemplate),
        desired: Desired,
        held: Option<ReplicaSetRead>,
    ) -> Served {
        let rendered = template.render(settings, &self.config.pod_keys());
        let held = match held {
            Some(ReplicaSetRead::Absent) => {
                self.create_replica_set(namespace, owner, desired.clamped, &rendered)
                    .await
            }
            other => {
                self.refusals.clear(namespace, CREATE_REPLICA_SET, "");
                other
            }
        };
        let hash = Some(rendered.hash.clone());
        let Some(ReplicaSetRead::Owned(replica_set)) = &held else {
            return Served { held, hash };
        };
        let template_applied = self
            .apply_template(namespace, owner, replica_set, &rendered)
            .await;
        let current = replicas_of(replica_set);
        let replicas = self
            .resize(namespace, owner, desired.clamped, current)
            .await;
        let target = target_of(namespace, replica_set);
        self.at_max_event(namespace, &target, settings, &desired)
            .await;
        if template_applied {
            self.roll(namespace, &rendered.hash, replicas).await;
        }
        Served { held, hash }
    }

    async fn create_replica_set(
        &mut self,
        namespace: &str,
        owner: &Owner,
        replicas: u32,
        rendered: &RenderedTemplate,
    ) -> Option<ReplicaSetRead> {
        if !self.leadership.is_leader() {
            return Some(ReplicaSetRead::Absent);
        }
        let result = self
            .writer
            .create_replica_set(namespace, owner, replicas, rendered)
            .await;
        self.note_write(namespace, CREATE_REPLICA_SET, "", &result);
        match result {
            Ok(outcome) => {
                tracing::info!(
                    pool = namespace,
                    ?outcome,
                    "the ReplicaSet of a pool was made"
                );
                self.observer.replica_set(namespace, owner).await.observed()
            }
            Err(error) => {
                tracing::warn!(pool = namespace, %error, "the ReplicaSet will be made again");
                self.retry_soon(namespace);
                Some(ReplicaSetRead::Absent)
            }
        }
    }

    async fn apply_template(
        &mut self,
        namespace: &str,
        owner: &Owner,
        replica_set: &ReplicaSet,
        rendered: &RenderedTemplate,
    ) -> bool {
        let key = template_hash_key(self.config.pod_keys().prefix());
        if template_in_place(replica_set, &key, &rendered.hash) {
            self.refusals.clear(namespace, APPLY_TEMPLATE, "");
            return true;
        }
        if !self.leadership.is_leader() {
            return false;
        }
        let result = self.writer.set_template(namespace, owner, rendered).await;
        self.note_write(namespace, APPLY_TEMPLATE, "", &result);
        match result {
            Ok(_) => {
                tracing::info!(
                    pool = namespace,
                    hash = rendered.hash,
                    "the template of a pool was changed"
                );
                true
            }
            Err(error) => {
                tracing::warn!(pool = namespace, %error, "the template will be written again");
                self.retry_soon(namespace);
                false
            }
        }
    }

    async fn remove_replica_set(
        &mut self,
        namespace: &str,
        owner: &Owner,
        held: Option<ReplicaSetRead>,
    ) -> Option<ReplicaSetRead> {
        if !matches!(held, Some(ReplicaSetRead::Owned(_))) {
            self.refusals.clear(namespace, DELETE_REPLICA_SET, "");
            return held;
        }
        if !self.leadership.is_leader() {
            return held;
        }
        let result = self.writer.delete_replica_set(namespace, owner).await;
        self.note_write(namespace, DELETE_REPLICA_SET, "", &result);
        match result {
            Ok(DeleteOutcome::Deleted | DeleteOutcome::Gone) => {
                tracing::warn!(
                    pool = namespace,
                    "the namespace has no fence, the ReplicaSet of the pool was deleted"
                );
                Some(ReplicaSetRead::Absent)
            }
            Ok(DeleteOutcome::Conflict) => {
                self.retry_soon(namespace);
                held
            }
            Err(error) => {
                tracing::warn!(pool = namespace, %error, "the ReplicaSet will be deleted again");
                self.retry_soon(namespace);
                held
            }
        }
    }

    pub(super) fn held_hash(&self, replica_set: &ReplicaSet) -> Option<String> {
        let key = template_hash_key(self.config.pod_keys().prefix());
        replica_set
            .spec
            .as_ref()?
            .template
            .as_ref()?
            .metadata
            .as_ref()?
            .annotations
            .as_ref()?
            .get(&key)
            .cloned()
    }
}
