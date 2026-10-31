use contract_jobs::graphql_roots::*;
use serde_json::Value;

use super::{TEXT, id, ids, jobs, new_mail, refusal};
use crate::backend::Backend;
use crate::error::{Error, Operation};
use crate::graphql;
use crate::model::{Content, NewMailInput, ResultRef};

fn content() -> Content {
    Content {
        message: "m".to_owned(),
        attachments: Vec::new(),
    }
}

fn mail() -> NewMailInput {
    new_mail(Vec::new())
}

macro_rules! refused {
    ($name:ident, $root:expr, $operation:expr, $reason:expr, |$b:ident, $w:ident| $call:expr) => {
        #[tokio::test]
        async fn $name() {
            let scripted = jobs(refusal($root, $reason));
            let ($b, $w) = (&*scripted as &dyn Backend, ids());
            let error = $call.await.unwrap_err();
            assert_eq!(error, Error::refused($operation, $reason, TEXT));
            assert!(error.is_refusal());
        }
    };
}

refused!(
    given_no_kept_payload_when_the_payload_is_read_then_it_is_refused_with_jobs_words,
    PAYLOAD_ROOT,
    Operation::Payload,
    "no_kept_payload",
    |b, w| graphql::payload(b, &w)
);
refused!(
    given_a_foreign_job_when_a_payload_is_uploaded_then_the_upload_is_refused,
    UPLOAD_PAYLOAD_ROOT,
    Operation::Upload,
    "job_exists_not_yours",
    |b, w| graphql::upload_payload(b, &w, id(9), &content())
);
refused!(
    given_a_second_payload_when_it_is_uploaded_then_payload_already_uploaded_is_kept,
    UPLOAD_PAYLOAD_ROOT,
    Operation::Upload,
    "payload_already_uploaded",
    |b, w| graphql::upload_payload(b, &w, id(9), &content())
);
refused!(
    given_an_ended_run_when_the_answer_is_uploaded_then_the_result_is_refused,
    UPLOAD_RESULT_ROOT,
    Operation::Result,
    "run_ended",
    |b, w| graphql::upload_result(b, &w, &content())
);
refused!(
    given_a_gone_result_when_it_is_read_then_the_result_is_refused,
    READ_RESULT_ROOT,
    Operation::Result,
    "result_unknown",
    |b, w| graphql::read_result(b, &w, ResultRef(id(5)), false)
);
refused!(
    given_a_result_for_another_runner_when_it_is_read_then_not_addressed_to_you_is_kept,
    READ_RESULT_ROOT,
    Operation::Result,
    "not_addressed_to_you",
    |b, w| graphql::read_result(b, &w, ResultRef(id(5)), true)
);
refused!(
    given_no_saved_state_when_it_is_read_then_the_read_is_refused,
    SAVED_STATE_ROOT,
    Operation::SavedState,
    "no_saved_state",
    |b, w| graphql::saved_state(b, &w)
);
refused!(
    given_a_large_state_when_it_is_saved_then_state_too_large_is_kept,
    SAVE_STATE_ROOT,
    Operation::SavedState,
    "state_too_large",
    |b, w| graphql::save_state(b, &w, &Value::Null)
);
refused!(
    given_a_blank_author_when_a_mail_is_sent_then_author_blank_is_kept,
    SEND_MAIL_ROOT,
    Operation::Mail,
    "author_blank",
    |b, w| graphql::send_mail(b, &w, &mail())
);
refused!(
    given_an_unknown_reason_when_a_mail_is_sent_then_the_word_is_not_changed,
    SEND_MAIL_ROOT,
    Operation::Mail,
    "a_reason_of_a_later_jobs",
    |b, w| graphql::send_mail(b, &w, &mail())
);
refused!(
    given_an_unknown_mail_when_it_is_read_then_the_read_is_refused,
    MAIL_ROOT,
    Operation::Read,
    "mail_not_yours",
    |b, w| graphql::read_mail(b, &w, id(3), id(4))
);
refused!(
    given_a_foreign_address_when_unread_is_read_then_the_read_is_refused,
    UNREAD_ROOT,
    Operation::Read,
    "address_not_yours",
    |b, w| graphql::unread(b, &w, Some(id(4)))
);
refused!(
    given_a_missing_attachment_when_it_is_fetched_then_the_attachment_is_refused,
    ATTACHMENT_ROOT,
    Operation::Attachment,
    "mail_not_yours",
    |b, w| graphql::attachment(b, &w, id(3), id(4))
);

#[tokio::test]
async fn given_a_refusal_of_the_connection_when_a_call_is_made_then_the_operation_of_the_call_is_named()
 {
    let scripted = crate::test_double::ScriptedJobs::new("runner", "instance-a");
    scripted.push_graphql(Err(Error::refused(Operation::Claim, "FORBIDDEN", TEXT)));
    let error = graphql::payload(&*scripted, &ids()).await.unwrap_err();
    assert_eq!(error, Error::refused(Operation::Payload, "FORBIDDEN", TEXT));
}

#[tokio::test]
async fn given_other_errors_of_the_connection_when_a_call_is_made_then_they_pass_unchanged() {
    for failure in [
        Error::Unauthenticated,
        Error::lost("cut"),
        Error::unavailable("down"),
    ] {
        let scripted = crate::test_double::ScriptedJobs::new("runner", "instance-a");
        scripted.push_graphql(Err(failure.clone()));
        let error = graphql::saved_state(&*scripted, &ids()).await.unwrap_err();
        assert_eq!(error, failure);
    }
}

#[tokio::test]
async fn given_a_refusal_when_it_is_lifted_then_no_call_is_repeated() {
    let scripted = jobs(refusal(PAYLOAD_ROOT, "no_kept_payload"));
    let _ = graphql::payload(&*scripted, &ids()).await;
    assert_eq!(super::sent(&scripted).len(), 1);
}
