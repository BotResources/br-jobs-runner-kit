use std::sync::Arc;
use std::time::Duration;

use crate::backend::{BoxFuture, ChildCommand, Fact};
use crate::error::Error;
use crate::graphql;
use crate::queue::{Write, WriteSender};
use crate::work::core::WorkCore;

pub(crate) struct WorkSender {
    pub core: Arc<WorkCore>,
    pub outcome_wait: Duration,
}

impl WriteSender for WorkSender {
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
                    backend.register_agents(*run_id, vec![agent.clone()], wait).await
                }
                Write::Mail { mail } => graphql::send_mail(backend, &ids, mail).await,
                Write::Marker {
                    run_id, item_id, ..
                } => backend.mark_read(*run_id, vec![*item_id], wait).await,
                Write::Upload { job_id, content } => {
                    graphql::upload_payload(backend, &ids, *job_id, content).await
                }
                Write::Create(create) => {
                    backend
                        .child_command(ChildCommand::Create(create.clone()), wait)
                        .await
                }
                Write::Finish {
                    run_id,
                    job_id,
                    command_id,
                } => {
                    backend
                        .child_command(
                            ChildCommand::Finish {
                                command_id: *command_id,
                                job_id: *job_id,
                                run_id: *run_id,
                            },
                            wait,
                        )
                        .await
                }
                Write::Cancel {
                    run_id,
                    job_id,
                    command_id,
                } => {
                    backend
                        .child_command(
                            ChildCommand::Cancel {
                                command_id: *command_id,
                                job_id: *job_id,
                                run_id: *run_id,
                            },
                            wait,
                        )
                        .await
                }
                Write::Answer(content) => graphql::upload_result(backend, &ids, content).await,
                Write::SavedState(state) => graphql::save_state(backend, &ids, state).await,
                Write::Fact(fact) => backend.publish_fact(fact.clone(), wait).await,
            }
        })
    }
}
