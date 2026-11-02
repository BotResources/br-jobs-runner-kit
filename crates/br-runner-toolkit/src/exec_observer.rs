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
                let held = self.shared.instances.insert(instance, &self.record);
                if !held {
                    tracing::warn!(
                        run_id = %self.record.run_id,
                        job_id = %self.record.job_id,
                        "an instance was reported after its run was dropped, it is not held by the lookup"
                    );
                }
                if self.record.phase() != Phase::Live {
                    hooks::call_once(&self.shared, &self.record, instance);
                }
            }
            Observation::EventHandled { instance, .. } => {
                hooks::call_once(&self.shared, &self.record, instance);
            }
            Observation::Outcome(_) => self.record.end_observations(),
            _ => {}
        }
    }
}

impl Drop for ExecObserver {
    fn drop(&mut self) {
        self.record.end_observations();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use br_graph_executor::{Observation, Observer};
    use br_runner_jobs::test_double::ScriptedJobs;

    use super::ExecObserver;
    use crate::hook::{InstanceEnded, lifecycle_hook};
    use crate::instances::Instances;
    use crate::record::Phase;
    use crate::unit_support::{
        book_with_agent, instance, queue_of, record, runner_type, shared, work,
    };

    #[tokio::test]
    async fn given_an_instance_reported_after_the_drain_aborted_its_run_when_it_is_observed_then_the_lookup_does_not_hold_it_and_the_hook_gets_the_agents_the_book_held()
     {
        let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
        let calls: Arc<Mutex<Vec<InstanceEnded>>> = Arc::default();
        let seen = Arc::clone(&calls);
        let hook = lifecycle_hook(move |ended| {
            let seen = Arc::clone(&seen);
            async move { seen.lock().expect("lock").push(ended) }
        });
        let instances = Arc::new(Instances::default());
        let shared = shared(&jobs, Some(hook), &instances);
        let work = work(&jobs, 1);
        let queue = queue_of(&work).await;
        let record = record(&work);
        record.set_book(Arc::new(book_with_agent(&work, &queue, "work")));
        instances.remove_run(&record, Phase::Aborted);
        drop(record.take_book());
        let observer = ExecObserver {
            shared,
            record: Arc::clone(&record),
        };
        observer.observe(Observation::InstanceCreated {
            instance: instance(7),
            node: "work".to_owned(),
            kind: "programmatic".to_owned(),
            subkind: "worker".to_owned(),
        });
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(!instances.holds(instance(7)));
        let calls = calls.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].node, "work");
        assert_eq!(calls[0].agents.len(), 1);
        assert_eq!(calls[0].agents[0].key.label, "main");
    }

    #[tokio::test]
    async fn given_an_instance_reported_while_its_run_is_live_when_it_is_observed_then_the_lookup_holds_it()
     {
        let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
        let instances = Arc::new(Instances::default());
        let shared = shared(&jobs, None, &instances);
        let work = work(&jobs, 1);
        let record = record(&work);
        let observer = ExecObserver {
            shared,
            record: Arc::clone(&record),
        };
        observer.observe(Observation::InstanceCreated {
            instance: instance(7),
            node: "work".to_owned(),
            kind: "programmatic".to_owned(),
            subkind: "worker".to_owned(),
        });
        assert!(instances.holds(instance(7)));
    }
}
