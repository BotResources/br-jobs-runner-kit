use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_jobs::{
    Cause, CredentialFailure, CredentialKind, Error, Failure, Operation, Refusal, RefusalReason,
    RunnerType,
};

#[test]
fn given_a_reason_a_message_and_an_operation_when_a_refusal_is_made_then_it_is_a_refused_error_with_the_typed_reason()
 {
    let refusal =
        Refusal::new(Operation::Create, "payload_too_large", "too big").with_retry_after_ms(250);
    assert_eq!(refusal.operation, Operation::Create);
    assert_eq!(refusal.reason, RefusalReason::PayloadTooLarge);
    assert_eq!(refusal.message, "too big");
    assert_eq!(refusal.retry_after_ms, Some(250));
    let error = Error::Refused(refusal);
    assert!(error.is_refusal());
    let direct = Error::refused(Operation::Create, RefusalReason::PayloadTooLarge, "too big");
    assert!(direct.is_refusal());
    assert!(direct.to_string().contains("payload_too_large"));
}

#[test]
fn given_a_word_the_library_does_not_know_when_a_refusal_is_made_then_the_text_is_kept() {
    let refusal = Refusal::new(Operation::Claim, "brand_new_reason", "");
    assert!(matches!(&refusal.reason, RefusalReason::Unknown(text) if text == "brand_new_reason"));
    assert_eq!(refusal.reason.as_str(), "brand_new_reason");
}

#[test]
fn given_a_cause_when_lost_and_unavailable_are_made_then_they_carry_it() {
    let lost = Error::Lost(Failure::new(Cause::WaitFired, "no outcome"));
    let unavailable = Error::Unavailable(Failure::new(Cause::NoListener, "nobody listens"));
    assert!(matches!(&lost, Error::Lost(failure) if failure.cause == Cause::WaitFired));
    assert!(
        matches!(&unavailable, Error::Unavailable(failure) if failure.cause == Cause::NoListener)
    );
    assert!(lost.to_string().contains("no outcome"));
}

#[test]
fn given_a_kind_when_a_credential_failure_is_made_then_it_is_a_credential_error() {
    let error = Error::Credential(CredentialFailure::new(
        CredentialKind::FetchFailed,
        "the vault is down",
    ));
    assert!(matches!(
        error,
        Error::Credential(failure) if failure.kind == CredentialKind::FetchFailed
    ));
}

#[test]
fn given_a_runner_type_the_wire_refuses_when_it_is_made_then_invalid_input_and_a_valid_one_names_the_client()
 {
    for value in ["", "a.b", "a b", "a*", "a>"] {
        assert!(matches!(
            RunnerType::new(value),
            Err(Error::InvalidInput(_))
        ));
    }
    let analyst = RunnerType::new("analyst").expect("a runner type");
    let jobs = ScriptedJobs::new(analyst.clone(), "pod-1");
    assert_eq!(jobs.client().runner_type(), &analyst);
}
