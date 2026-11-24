use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use br_agentic_contract::{AgentLabel, IncomingContext, IncomingItem, ThreadRef};
use br_graph_contract::InstanceId;
use br_llm_messages::{AgentId, Author, IncomingEntry, InputTurn, RuntimeNotice, TurnId};
use br_runner_threads::ThreadClient;
use br_runner_toolkit::InstanceLookup;
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::threads::{AgentCell, thread_adapter};
use crate::wiring::{Variant, Wired};

pub type Seeder = Arc<dyn Fn(AgentId) + Send + Sync>;

#[derive(Default)]
pub struct Observed {
    pub written: AtomicUsize,
    pub outcomes: Mutex<Vec<String>>,
}

impl Observed {
    pub fn outcomes(&self) -> Vec<String> {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub struct MailNode {
    pub lookup: Arc<OnceLock<InstanceLookup>>,
    pub threads: ThreadClient,
    pub variant: Variant,
    pub seeder: Seeder,
    pub patience: Duration,
    pub observed: Arc<Observed>,
}

fn limit() -> NonZeroUsize {
    NonZeroUsize::MIN.saturating_add(19)
}

impl MailNode {
    pub async fn work(&self, instance: InstanceId) -> Result<String, String> {
        let lookup = self.lookup.get().ok_or("the lookup is not set")?;
        let label = AgentLabel::new("analyst").map_err(|e| e.to_string())?;
        let cell = AgentCell::default();
        let author = Author::new("Analyst").map_err(|e| e.to_string())?;
        let threads = thread_adapter(&self.threads, Arc::clone(&cell), author)
            .await
            .map_err(|e| e.to_string())?;
        let wired = Wired::build(self.variant, lookup.book_lookup(), label.clone(), threads)
            .ok_or("the entry gives no inbox")?;
        let book = lookup.book(instance).await.map_err(|e| e.to_string())?;
        let agent = wired
            .entry
            .agent_id(&book, &label)
            .map_err(|e| e.to_string())?;
        cell.set(agent).map_err(|_| "the agent was set twice")?;
        (self.seeder)(agent);
        let thread = ThreadRef::from_uuid(Uuid::now_v7()).map_err(|e| e.to_string())?;
        let context = IncomingContext {
            instance,
            thread,
            agent,
        };
        self.take_mail(&wired, &context).await?;
        self.wait_until_read(&wired, &context).await
    }

    async fn take_mail(&self, wired: &Wired, context: &IncomingContext) -> Result<(), String> {
        let deadline = Instant::now() + self.patience;
        let batch = loop {
            let batch = wired
                .incoming
                .items(context, None, limit())
                .await
                .map_err(|e| e.to_string())?;
            if !batch.items.is_empty() {
                break batch;
            }
            if Instant::now() >= deadline {
                return Err("no mail arrived".to_owned());
            }
            sleep(Duration::from_millis(10)).await;
        };
        for item in &batch.items {
            let IncomingItem::Mail(mail) = item else {
                continue;
            };
            let notice =
                RuntimeNotice::new(item.id(), mail.subject()).map_err(|e| e.to_string())?;
            let turn = InputTurn::new(
                TurnId::from_uuid(Uuid::now_v7()).map_err(|e| e.to_string())?,
                vec![IncomingEntry::from(notice)],
            )
            .map_err(|e| e.to_string())?;
            wired
                .history
                .add_input_turn(&context.thread, turn)
                .await
                .map_err(|e| e.to_string())?;
            self.observed.written.fetch_add(1, Ordering::SeqCst);
            wired
                .incoming
                .mark_read(context, item.id())
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    async fn wait_until_read(
        &self,
        wired: &Wired,
        context: &IncomingContext,
    ) -> Result<String, String> {
        let deadline = Instant::now() + self.patience;
        loop {
            let left = wired
                .incoming
                .items(context, None, limit())
                .await
                .map_err(|e| e.to_string())?
                .items
                .len();
            if left == 0 {
                return Ok("read".to_owned());
            }
            if Instant::now() >= deadline {
                return Ok(format!("{left} unread"));
            }
            sleep(Duration::from_millis(10)).await;
        }
    }
}
