use br_llm_messages::{AgentId, Conversation, ProjectedThread};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::error::Error;

#[derive(Debug, Clone)]
pub struct ContextQuery {
    pub thread_id: Uuid,
    pub agent: Option<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub in_roster: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Resource {
    pub key: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ContextContent {
    Projected(ProjectedThread),
    Conversation(Conversation),
}

#[derive(Debug, Clone)]
pub struct ThreadContext {
    pub thread_id: Uuid,
    pub content: ContextContent,
    pub members: Vec<Member>,
    pub resources: Vec<Resource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire {
    thread_id: Uuid,
    conversation: Option<Value>,
    projected_thread: Option<Value>,
    members: Vec<Member>,
    resources: Vec<Resource>,
}

pub(crate) fn parse(value: Value, agent_named: bool) -> Result<Option<ThreadContext>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    let wire: Wire = serde_json::from_value(value)
        .map_err(|e| Error::Unreadable(format!("the context is malformed: {e}")))?;
    let content = match (agent_named, wire.projected_thread, wire.conversation) {
        (true, Some(projected), _) => ContextContent::Projected(
            ProjectedThread::from_json(&text(&projected)?).map_err(unreadable)?,
        ),
        (false, _, Some(conversation)) => ContextContent::Conversation(
            Conversation::from_json(&text(&conversation)?).map_err(unreadable)?,
        ),
        _ => {
            return Err(Error::Unreadable(
                "the context holds no document for the request".to_owned(),
            ));
        }
    };
    Ok(Some(ThreadContext {
        thread_id: wire.thread_id,
        content,
        members: wire.members,
        resources: wire.resources,
    }))
}

fn text(document: &Value) -> Result<String, Error> {
    serde_json::to_string(document).map_err(|e| Error::Unreadable(e.to_string()))
}

fn unreadable(error: br_llm_messages::Error) -> Error {
    Error::Unreadable(error.code().as_str().to_owned())
}
