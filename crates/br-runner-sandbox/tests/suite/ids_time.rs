use std::time::{Duration, SystemTime, UNIX_EPOCH};

use br_runner_sandbox::{AttemptId, ClientId, InvalidClientId, Timestamp};
use uuid::Uuid;

#[test]
fn a_client_id_is_not_blank_and_holds_no_slash() {
    assert_eq!(ClientId::new(" "), Err(InvalidClientId::Blank));
    assert_eq!(ClientId::new("a/b"), Err(InvalidClientId::Slash));
    assert_eq!(ClientId::new("acme").expect("client").as_str(), "acme");
}

#[test]
fn an_attempt_id_is_a_uuid_v7_in_hyphenated_lowercase_text() {
    let attempt = AttemptId::mint();
    let text = attempt.to_string();
    assert_eq!(AttemptId::parse(&text), Ok(attempt));
    assert!(AttemptId::parse(&text.to_uppercase()).is_err());
    assert!(
        AttemptId::parse(&Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef).to_string())
            .is_err()
    );
    assert!(AttemptId::parse("not a uuid").is_err());
    assert!(AttemptId::parse(&attempt.as_uuid().simple().to_string()).is_err());
}

#[test]
fn two_attempts_are_two_ids() {
    assert_ne!(AttemptId::mint(), AttemptId::mint());
}

#[test]
fn a_timestamp_is_rfc_3339_utc_with_milliseconds() {
    let time = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
    let stamp = Timestamp::from_system_time(time);
    assert_eq!(stamp.to_string(), "2023-11-14T22:13:20.123Z");
    assert_eq!(Timestamp::parse(&stamp.to_string()), Ok(stamp));
    assert_eq!(
        stamp.to_system_time(),
        UNIX_EPOCH + Duration::new(1_700_000_000, 123_000_000)
    );
}

#[test]
fn a_timestamp_read_with_an_offset_is_the_same_instant_in_utc() {
    let stamp = Timestamp::parse("2023-11-14T23:13:20.123+01:00").expect("time");
    assert_eq!(stamp.to_string(), "2023-11-14T22:13:20.123Z");
    assert!(Timestamp::parse("yesterday").is_err());
}

#[test]
fn a_time_has_passed_at_its_instant_and_after() {
    let base = Timestamp::from_system_time(UNIX_EPOCH + Duration::from_secs(1000));
    let deadline = base.plus(Duration::from_secs(10));
    assert!(!deadline.has_passed(base));
    assert!(deadline.has_passed(deadline));
    assert!(deadline.has_passed(deadline.plus(Duration::from_millis(1))));
}

#[test]
fn a_huge_span_does_not_overflow() {
    let now = Timestamp::now();
    assert!(!now.plus(Duration::MAX).has_passed(now));
    assert!(SystemTime::now() >= now.to_system_time() - Duration::from_secs(1));
}
