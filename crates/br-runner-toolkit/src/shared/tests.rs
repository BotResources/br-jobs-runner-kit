use br_runner_jobs::test_double::ScriptedJobs;
use std::sync::Arc;

use super::Mode;
use crate::instances::Instances;
use crate::unit_support::{id, runner_type, shared};

fn fresh() -> Arc<super::Shared> {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
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

#[test]
fn given_a_run_in_flight_when_the_orders_are_replaced_by_a_list_with_its_order_then_its_token_is_triggered_and_the_token_of_another_run_is_not()
 {
    let shared = fresh();
    let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
    let (_, other) = shared.register_run(id(3), id(4)).ok().expect("registered");
    shared.replace_orders(std::collections::HashSet::from([id(1)]));
    assert!(token.handle().is_triggered());
    assert!(!other.handle().is_triggered());
}

#[test]
fn given_a_run_with_a_triggered_token_when_the_orders_are_replaced_by_a_list_without_its_order_then_the_token_stays_triggered()
 {
    let shared = fresh();
    let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
    shared.replace_orders(std::collections::HashSet::from([id(1)]));
    shared.replace_orders(std::collections::HashSet::new());
    assert!(token.handle().is_triggered());
}

#[test]
fn given_orders_replaced_by_a_list_when_a_run_registers_then_it_starts_cancelled_only_if_the_list_holds_its_order()
 {
    let shared = fresh();
    shared.replace_orders(std::collections::HashSet::from([id(1)]));
    let (_, ordered) = shared.register_run(id(1), id(2)).ok().expect("registered");
    let (_, free) = shared.register_run(id(3), id(4)).ok().expect("registered");
    assert!(ordered.handle().is_triggered());
    assert!(!free.handle().is_triggered());
}
