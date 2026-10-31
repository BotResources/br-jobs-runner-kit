use std::sync::Arc;

use br_graph_executor::{Observation, Observer};

use crate::hooks;
use crate::record::{Phase, RunRecord};
use crate::shared::Shared;

pub(crate) struct ExecObserver {
    pub shared: Arc<Shared>,
    pub record: Arc<RunRecord>,
}

impl Observer for ExecObserver {
    fn observe(&self, observation: Observation) {
        match observation {
            Observation::InstanceCreated { instance, node, .. } => {
                self.record.instance_created(instance, node);
                self.shared
                    .instances
                    .insert(instance, Arc::clone(&self.record));
                if self.record.phase() != Phase::Live {
                    hooks::call_once(&self.shared, &self.record, instance);
                }
            }
            Observation::EventHandled { instance, .. } => {
                hooks::call_once(&self.shared, &self.record, instance);
            }
            _ => {}
        }
    }
}
