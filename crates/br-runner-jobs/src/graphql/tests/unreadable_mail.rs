use contract_jobs::graphql_roots::MAIL_ROOT;
use serde_json::{Value, json};

use super::unreadable::assert_unreadable;
use super::{id, ids, jobs, success};
use crate::graphql;

#[tokio::test]
async fn given_a_mail_view_without_author_or_with_bad_lists_then_the_read_is_unreadable() {
    for (key, value) in [
        ("author", Value::Null),
        ("to", json!("x")),
        ("cc", Value::Null),
        ("from", json!("not an id")),
        ("attachments", json!([{ "name": "n" }])),
    ] {
        let mut view = json!({
            "mailId": id(3), "from": id(4), "author": "Ann", "sourceConversationId": id(5),
            "subject": "s", "body": "b", "to": [], "cc": [], "attachments": [],
        });
        view[key] = value;
        let scripted = jobs(success(MAIL_ROOT, json!({ "mail": view })));
        let error = graphql::read_mail(&*scripted, &ids(), id(3), id(4))
            .await
            .unwrap_err();
        assert_unreadable(&error);
    }
}
