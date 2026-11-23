use serde_json::{Value, json};

use super::lift::{call, unreadable};
use super::{WorkIds, decode, documents};
use crate::backend::Backend;
use crate::error::{Error, Operation};
use crate::model::SavedState;
use contract_jobs::graphql_roots::{SAVE_STATE_ROOT, SAVED_STATE_ROOT};

pub(crate) async fn save_state(
    backend: &dyn Backend,
    work: &WorkIds,
    state: &Value,
) -> Result<(), Error> {
    let variables = json!({ "input": { "work": work.json(), "state": state } });
    call(
        backend,
        Operation::SavedState,
        SAVE_STATE_ROOT,
        &documents::SAVE_STATE,
        variables,
    )
    .await
    .map(drop)
}

pub(crate) async fn saved_state(
    backend: &dyn Backend,
    work: &WorkIds,
) -> Result<SavedState, Error> {
    let variables = json!({ "work": work.json() });
    let root = call(
        backend,
        Operation::SavedState,
        SAVED_STATE_ROOT,
        &documents::SAVED_STATE,
        variables,
    )
    .await?;
    Ok(SavedState {
        state: root
            .get("state")
            .cloned()
            .ok_or_else(|| unreadable("state"))?,
        agent_ids: decode::uuids(&root, "agentIds")?,
        mail_addresses: decode::uuids(&root, "mailAddresses")?,
    })
}
