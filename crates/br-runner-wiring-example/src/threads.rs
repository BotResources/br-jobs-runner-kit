use std::sync::{Arc, OnceLock};
use std::time::Duration;

use br_agentic_contract::PortError;
use br_llm_messages::{AgentId, Author};
use br_runner_threads::{
    Error, IgnoreReports, MemoryQueueStore, ThreadAccessAdapter, ThreadAccessSettings,
    ThreadClient, WriteContext, WriteContextFn,
};

pub type AgentCell = Arc<OnceLock<AgentId>>;

pub async fn thread_adapter(
    client: &ThreadClient,
    agent: AgentCell,
    author: Author,
) -> Result<Arc<ThreadAccessAdapter>, Error> {
    let write_context = WriteContextFn::new(move |_thread| {
        let known = agent.get().copied();
        let author = author.clone();
        async move {
            let agent = known.ok_or_else(|| PortError::fault("the agent is not known yet"))?;
            Ok::<_, PortError>(WriteContext {
                agent,
                author,
                job_id: None,
                acting_user_id: None,
                impersonator_id: None,
            })
        }
    });
    let settings = ThreadAccessSettings {
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_millis(20),
        keepalive_interval: Duration::from_secs(3600),
        store: Arc::new(MemoryQueueStore::new()),
        observer: Arc::new(IgnoreReports),
        write_context,
    };
    Ok(Arc::new(ThreadAccessAdapter::new(client, settings).await?))
}
