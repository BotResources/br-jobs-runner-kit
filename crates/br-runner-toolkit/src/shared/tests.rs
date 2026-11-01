use br_runner_jobs::test_double::ScriptedJobs;
use std::sync::Arc;

use super::Mode;
use crate::instances::Instances;
use crate::unit_support::{id, shared};

fn fresh() -> Arc<super::Shared> {
    let jobs = ScriptedJobs::new("analyst", "i1");
    shared(&jobs, None, &Arc::new(Instances::default()))
}

#[test]
fn given_a_stop_decided_before_a_mode_change_when_it_is_applied_late_then_the_claims_are_not_stopped()
 {
    let shared = fresh();
    let epoch = shared.mode_epoch();
    shared.mode_changed(Mode::Claim);
    shared.stop_claims(epoch);
    assert!(!shared.claims_stopped());
}

#[test]
fn given_a_stop_decided_in_the_current_mode_when_the_mode_changes_then_the_claims_are_open_again() {
    let shared = fresh();
    shared.mode_changed(Mode::Claim);
    shared.stop_claims(shared.mode_epoch());
    assert!(shared.claims_stopped());
    shared.mode_changed(Mode::Claim);
    assert!(!shared.claims_stopped());
}

#[test]
fn given_an_order_made_and_deleted_before_the_run_exists_when_the_run_registers_then_it_starts_live()
 {
    let shared = fresh();
    shared.order_made(id(1));
    shared.order_deleted(id(1));
    let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
    assert!(!token.handle().is_triggered());
}

#[test]
fn given_an_order_made_for_a_registered_run_when_the_order_is_deleted_then_the_token_stays_triggered()
 {
    let shared = fresh();
    let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
    shared.order_made(id(1));
    shared.order_deleted(id(1));
    assert!(token.handle().is_triggered());
}

#[test]
fn given_an_order_made_before_the_run_exists_when_the_run_registers_then_it_starts_cancelled() {
    let shared = fresh();
    shared.order_made(id(1));
    let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
    assert!(token.handle().is_triggered());
}

#[test]
fn given_a_run_already_registered_when_the_same_run_registers_again_then_the_first_record_is_returned()
 {
    let shared = fresh();
    let (first, _) = shared.register_run(id(1), id(2)).ok().expect("registered");
    let existing = shared.register_run(id(1), id(2)).err().expect("refused");
    assert!(Arc::ptr_eq(&first, &existing));
}
