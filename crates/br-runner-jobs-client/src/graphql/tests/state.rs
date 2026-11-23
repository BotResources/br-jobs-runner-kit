use contract_jobs::graphql_roots::*;
use serde_json::json;

use super::{id, ids, jobs, success};
use crate::graphql;

#[tokio::test]
async fn given_a_kept_state_when_it_is_read_then_state_agent_ids_and_mail_addresses_are_returned() {
    let state = json!({ "child_jobs": ["x"], "n": 3 });
    let scripted = jobs(success(
        SAVED_STATE_ROOT,
        json!({ "state": state, "agentIds": [id(7), id(8)], "mailAddresses": [id(7)] }),
    ));
    let saved = graphql::saved_state(&*scripted, &ids()).await.unwrap();
    assert_eq!(saved.state, state);
    assert_eq!(saved.agent_ids, vec![id(7), id(8)]);
    assert_eq!(saved.mail_addresses, vec![id(7)]);
}

#[tokio::test]
async fn given_a_state_saved_when_jobs_accepts_then_the_save_is_ok() {
    let scripted = jobs(success(SAVE_STATE_ROOT, json!({})));
    graphql::save_state(&*scripted, &ids(), &json!({ "a": 1 }))
        .await
        .unwrap();
}
