use serde_json::Value;

use super::Call;
use super::ScriptedJobs;
use crate::error::Error;
use crate::model::ListedAgent;

impl ScriptedJobs {
    pub fn calls(&self) -> Vec<Call> {
        self.script().calls.clone()
    }

    pub fn push_graphql(&self, answer: Result<Value, Error>) {
        self.script().graphql.push_back(answer);
    }

    pub fn graphql_default(&self, answer: Value) {
        self.script().graphql_default = Some(answer);
    }

    pub fn answer_facts(&self, answer: impl FnMut() -> Result<(), Error> + Send + 'static) {
        self.script().facts = Some(Box::new(answer));
    }

    pub fn answer_started(&self, answer: impl FnMut() -> Result<(), Error> + Send + 'static) {
        self.script().started = Some(Box::new(answer));
    }

    pub fn answer_register(&self, answer: impl FnMut() -> Result<(), Error> + Send + 'static) {
        self.script().register = Some(Box::new(answer));
    }

    pub fn answer_mark_read(&self, answer: impl FnMut() -> Result<(), Error> + Send + 'static) {
        self.script().mark_read = Some(Box::new(answer));
    }

    pub fn answer_commands(&self, answer: impl FnMut() -> Result<(), Error> + Send + 'static) {
        self.script().command = Some(Box::new(answer));
    }

    pub fn set_agents(&self, agents: Vec<ListedAgent>) {
        self.script().agents = agents;
    }

    pub fn pause(&self) {
        *self.hold.lock().unwrap_or_else(|p| p.into_inner()) = true;
    }

    pub fn resume(&self) {
        *self.hold.lock().unwrap_or_else(|p| p.into_inner()) = false;
        self.released.notify_waiters();
    }
}
