use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use br_graph_executor::Observer;
use uuid::Uuid;

use crate::core::Core;
use crate::saved_state::{SavedAgent, SavedState, SavedStateError};

pub struct RunHandle {
    core: Arc<Core>,
    run: Uuid,
    ended: AtomicBool,
}

impl RunHandle {
    pub(crate) fn begin(
        core: Arc<Core>,
        run: Uuid,
        restored: Option<&SavedState>,
    ) -> Result<Self, SavedStateError> {
        let loaded = restored
            .map(SavedState::keyed)
            .transpose()?
            .unwrap_or_default();
        core.runs.begin(run);
        for (key, agent, inbox) in loaded {
            core.runs.preload(run, key, agent, inbox);
        }
        Ok(Self {
            core,
            run,
            ended: AtomicBool::new(false),
        })
    }

    pub fn observer(&self) -> Arc<dyn Observer> {
        let core = Arc::clone(&self.core);
        self.core.runs.observer(
            self.run,
            Arc::new(move |ended| {
                let agents: Vec<_> = core
                    .runs
                    .agents_of(ended.run)
                    .into_iter()
                    .filter(|(key, _, _)| key.node == ended.node)
                    .map(|(_, agent, _)| agent)
                    .collect();
                core.drop_objects_of(&agents);
            }),
        )
    }

    pub fn agents(&self) -> Vec<SavedAgent> {
        self.core
            .runs
            .agents_of(self.run)
            .into_iter()
            .map(|(key, agent, inbox)| SavedAgent {
                node: key.node,
                label: key.label.to_string(),
                agent: agent.as_uuid().to_string(),
                inbox,
            })
            .collect()
    }

    pub fn has_registered_inbox_agent(&self) -> bool {
        self.agents().iter().any(|agent| agent.inbox)
    }

    pub fn end(&self) {
        if self.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        let agents: Vec<_> = self
            .core
            .runs
            .agents_of(self.run)
            .into_iter()
            .map(|(_, agent, _)| agent)
            .collect();
        self.core.runs.end(self.run);
        self.core.streams.forget_run(self.run);
        self.core.drop_objects_of(&agents);
    }
}

impl Drop for RunHandle {
    fn drop(&mut self) {
        self.end();
    }
}
