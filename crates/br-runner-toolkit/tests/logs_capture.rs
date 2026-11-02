mod support;

use support::Logs;
use tracing::Level;

fn emit() {
    tracing::error!("a line of the test");
}

#[test]
fn given_a_thread_without_capture_that_hit_the_event_first_when_a_test_captures_then_only_its_own_line_is_captured()
 {
    let (logs, _guard) = Logs::capture();
    std::thread::spawn(emit).join().expect("joined");
    emit();
    assert_eq!(logs.containing(Level::ERROR, "a line of the test"), 1);
}

#[test]
fn given_two_captures_in_two_threads_when_both_log_then_each_one_holds_only_its_own_line() {
    let other = std::thread::spawn(|| {
        let (logs, _guard) = Logs::capture();
        emit();
        emit();
        logs.containing(Level::ERROR, "a line of the test")
    });
    let (logs, _guard) = Logs::capture();
    emit();
    assert_eq!(other.join().expect("joined"), 2);
    assert_eq!(logs.containing(Level::ERROR, "a line of the test"), 1);
}
