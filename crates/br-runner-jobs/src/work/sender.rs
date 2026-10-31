use std::sync::Arc;
use std::time::Duration;

use crate::backend::{BoxFuture, ChildCommand, ChildSession, Fact};
use crate::error::Error;
use crate::graphql;
use crate::queue::{Direct, Write, WriteSender, WriteSession};
use crate::work::core::WorkCore;

pub(crate) struct WorkSender {
    pub core: Arc<WorkCore>,
    pub outcome_wait: Duration,
}

struct ChildWrite<'a> {
    session: Box<dyn ChildSession + 'a>,
    wait: Duration,
}

impl WriteSession for ChildWrite<'_> {
    fn send(&mut self) -> BoxFuture<'_, Result<(), Error>> {
        self.session.attempt(self.wait)
    }
}

impl WorkSender {
    fn child_command(write: &Write) -> Option<ChildCommand> {
        match write {
            Write::Create(create) => Some(ChildCommand::Create(create.clone())),
            Write::Finish {
                run_id,
                job_id,
                command_id,
            } => Some(ChildCommand::Finish {
                command_id: *command_id,
                job_id: *job_id,
                run_id: *run_id,
            }),
            Write::Cancel {
                run_id,
                job_id,
                command_id,
            } => Some(ChildCommand::Cancel {
                command_id: *command_id,
                job_id: *job_id,
                run_id: *run_id,
            }),
            _ => None,
        }
    }
}

impl WriteSender for WorkSender {
    fn session<'a>(&'a self, write: &'a Write) -> Box<dyn WriteSession + 'a> {
        match Self::child_command(write) {
            Some(command) => Box::new(ChildWrite {
                session: self.core.backend.child_session(command),
                wait: self.outcome_wait,
            }),
            None => Box::new(Direct {
                sender: self,
                write,
            }),
        }
    }

    fn send<'a>(&'a self, write: &'a Write) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            let backend = self.core.backend.as_ref();
            let ids = self.core.ids();
            let wait = self.outcome_wait;
            match write {
                Write::Started { run_id, start_id } => {
                    backend
                        .publish_fact(
                            Fact::Started {
                                run_id: *run_id,
                                start_id: *start_id,
                            },
                            wait,
                        )
                        .await
                }
                Write::Registration { run_id, agent } => {
                    backend
                        .register_agents(*run_id, vec![agent.clone()], wait)
                        .await
                }
                Write::Mail { mail } => graphql::send_mail(backend, &ids, mail).await,
                Write::Marker {
                    run_id, item_id, ..
                } => backend.mark_read(*run_id, vec![*item_id], wait).await,
                Write::Upload { job_id, content } => {
                    graphql::upload_payload(backend, &ids, *job_id, content).await
                }
                Write::Create(_) | Write::Finish { .. } | Write::Cancel { .. } => {
                    self.session(write).send().await
                }
                Write::Answer(content) => graphql::upload_result(backend, &ids, content).await,
                Write::SavedState(state) => graphql::save_state(backend, &ids, state).await,
                Write::Fact(fact) => backend.publish_fact(fact.clone(), wait).await,
            }
        })
    }
}
