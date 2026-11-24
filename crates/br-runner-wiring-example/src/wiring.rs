use std::sync::Arc;

use br_agentic_contract::{AgentLabel, IncomingPort, ThreadAccess};
use br_agentic_harness::Harness;
use br_agentic_harness_contract::{HarnessHistory, MailPort};
use br_runner_jobs::NodeEntry;
use br_runner_jobs_client::BookLookup;
use br_runner_threads::ThreadAccessAdapter;

use crate::fields::Report;
use crate::gate::thread_mark_gate;
use crate::ports::{NoTools, StaticPrompt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Harness,
    Gate,
}

pub struct Wired {
    pub entry: NodeEntry,
    pub incoming: Arc<dyn IncomingPort>,
    pub history: Arc<dyn ThreadAccess>,
}

impl Wired {
    pub fn build(
        variant: Variant,
        lookup: Arc<dyn BookLookup>,
        label: AgentLabel,
        threads: Arc<ThreadAccessAdapter>,
    ) -> Option<Self> {
        match variant {
            Variant::Harness => harness(lookup, label, threads),
            Variant::Gate => gate(lookup, label, threads),
        }
    }
}

fn harness(
    lookup: Arc<dyn BookLookup>,
    label: AgentLabel,
    threads: Arc<ThreadAccessAdapter>,
) -> Option<Wired> {
    let entry = NodeEntry::with_mail("analyst", lookup, label);
    let mail: Arc<dyn MailPort> = Arc::new(entry.mail_port()?);
    let history: Arc<dyn HarnessHistory> = threads;
    let harness = Harness::<Report>::builder(
        history,
        Arc::new(StaticPrompt("You are the analyst.".to_owned())),
        Arc::new(NoTools),
    )
    .mail(mail)
    .build();
    Some(Wired {
        incoming: harness.incoming()?,
        history: harness.history(),
        entry,
    })
}

fn gate(
    lookup: Arc<dyn BookLookup>,
    label: AgentLabel,
    threads: Arc<ThreadAccessAdapter>,
) -> Option<Wired> {
    let entry = NodeEntry::with_inbox(
        "analyst",
        lookup,
        label,
        thread_mark_gate(Arc::clone(&threads)),
    );
    Some(Wired {
        incoming: Arc::new(entry.incoming_port()?),
        history: threads,
        entry,
    })
}
