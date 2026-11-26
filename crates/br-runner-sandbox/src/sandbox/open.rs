use br_agentic_harness_contract::{FreshReason, OpenRequest, Opened, Seed, StartKind, TurnRef};

use crate::decide::{StartPlan, check_web_access, plan_start};
use crate::error::Error;
use crate::flow::RestoreVerdict;
use crate::formats::Manifest;
use crate::ids::SaveKey;
use crate::seed::{seed_archive, seed_hash_text};

use super::guard::OpenGuard;
use super::live::Live;
use super::state::Sandbox;

enum Plan {
    Live(String),
    Restore(Box<Manifest>),
    Fresh(FreshReason),
}

impl Sandbox {
    pub async fn open_box(&self, request: &OpenRequest) -> Result<Opened, Error> {
        let mut guard = OpenGuard::take(&self.inner, request.instance)?;
        check_web_access(self.env().config.network(), request.web_access.as_ref())?;
        seed_archive(&request.seed)?;
        let turn = request.turn;
        let seed_hash = seed_hash_text(&request.seed);
        let (pod, start) = match self.choose_start(&turn, &seed_hash, &mut guard).await? {
            Plan::Live(pod) => (pod, StartKind::Continued),
            Plan::Fresh(reason) => {
                let pod = self
                    .claim_and_seed(&turn, &request.seed, &mut guard)
                    .await?;
                (pod, StartKind::Fresh(reason))
            }
            Plan::Restore(manifest) => {
                self.restore_or_fresh(&turn, &request.seed, &manifest, &mut guard)
                    .await?
            }
        };
        guard.commit(pod.clone(), turn);
        Ok(Opened::new(
            br_agentic_harness_contract::BoxId::new(pod),
            start,
        ))
    }

    async fn choose_start(
        &self,
        turn: &TurnRef,
        seed_hash: &str,
        guard: &mut OpenGuard,
    ) -> Result<Plan, Error> {
        let env = self.env();
        let Some(parent) = turn.parent else {
            return Ok(Plan::Fresh(FreshReason::NoSaveEver));
        };
        match self.reuse_live(turn, parent, seed_hash, guard).await? {
            Live::Claimed(pod) => Ok(Plan::Live(pod)),
            Live::Expired(expired_at) => Ok(Plan::Fresh(crate::decide::live_expired(expired_at))),
            Live::Nothing => {
                let key = SaveKey::new(
                    env.config.client_id().clone(),
                    turn.thread,
                    turn.agent,
                    parent,
                );
                let view = env.read_store_view(&key).await;
                Ok(
                    match plan_start(view, parent, env.clock.now(), env.config.save_ttl()) {
                        StartPlan::Restore(manifest) => Plan::Restore(manifest),
                        StartPlan::Fresh(reason) => Plan::Fresh(reason),
                    },
                )
            }
        }
    }

    async fn claim_and_seed(
        &self,
        turn: &TurnRef,
        seed: &Seed,
        guard: &mut OpenGuard,
    ) -> Result<String, Error> {
        let pod = self.claim_pod(turn, guard).await?;
        self.seed_or_discard(&pod, seed, guard).await?;
        Ok(pod)
    }

    async fn seed_or_discard(
        &self,
        pod: &str,
        seed: &Seed,
        guard: &mut OpenGuard,
    ) -> Result<(), Error> {
        let written = self.env().write_seed(pod, seed).await;
        if written.is_err() {
            self.discard(guard).await;
        }
        written
    }

    async fn restore_or_fresh(
        &self,
        turn: &TurnRef,
        seed: &Seed,
        manifest: &Manifest,
        guard: &mut OpenGuard,
    ) -> Result<(String, StartKind), Error> {
        let env = self.env();
        let pod = self.claim_pod(turn, guard).await?;
        let parent = manifest.key().turn();
        let (restored, seeded) = tokio::join!(
            env.restore(&pod, manifest, parent),
            env.write_seed(&pod, seed)
        );
        match restored {
            RestoreVerdict::Restored => {
                if seeded.is_err() {
                    self.discard(guard).await;
                }
                seeded?;
                Ok((pod, StartKind::Restored { turn: parent }))
            }
            RestoreVerdict::Fresh(reason) => {
                self.discard(guard).await;
                let pod = self.claim_and_seed(turn, seed, guard).await?;
                Ok((pod, StartKind::Fresh(reason)))
            }
        }
    }
}
