use std::time::Duration;

use br_runner_toolkit::{ErrorCode, Settings, SettingsBuilder};

fn complete() -> SettingsBuilder {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(2)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
}

#[test]
fn given_every_setting_when_built_then_the_getters_give_the_values() {
    let settings = complete().build().expect("settings");
    assert_eq!(settings.runner_version(), "1.0.0");
    assert_eq!(settings.max_concurrent_runs(), 2);
    assert_eq!(settings.ack_wait(), Duration::from_secs(30));
    assert_eq!(settings.drain_timeout(), Duration::from_secs(60));
    assert_eq!(settings.cancel_grace(), Duration::from_secs(1));
    assert_eq!(settings.claim_wait(), Duration::from_secs(5));
    assert_eq!(settings.outcome_wait(), Duration::from_secs(10));
    assert_eq!(settings.retry_wait(), Duration::from_secs(1));
}

#[test]
fn given_no_setting_when_built_then_the_error_is_setting_missing() {
    let error = Settings::builder().build().expect_err("an error");
    assert_eq!(error.code(), ErrorCode::SettingMissing);
}

#[test]
fn given_no_runner_version_when_built_then_the_error_is_setting_missing() {
    let builder = Settings::builder()
        .max_concurrent_runs(1)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1));
    assert_eq!(
        builder.build().expect_err("an error").code(),
        ErrorCode::SettingMissing
    );
}

#[test]
fn given_zero_slots_when_built_then_the_error_is_setting_invalid() {
    let error = complete()
        .max_concurrent_runs(0)
        .build()
        .expect_err("an error");
    assert_eq!(error.code(), ErrorCode::SettingInvalid);
}

#[test]
fn given_a_zero_wait_when_built_then_the_error_is_setting_invalid() {
    for builder in [
        complete().ack_wait(Duration::ZERO),
        complete().claim_wait(Duration::ZERO),
        complete().outcome_wait(Duration::ZERO),
        complete().retry_wait(Duration::ZERO),
    ] {
        assert_eq!(
            builder.build().expect_err("an error").code(),
            ErrorCode::SettingInvalid
        );
    }
}

#[test]
fn given_a_zero_drain_timeout_and_a_zero_cancel_grace_when_built_then_the_settings_are_valid() {
    let settings = complete()
        .drain_timeout(Duration::ZERO)
        .cancel_grace(Duration::ZERO)
        .build()
        .expect("settings");
    assert_eq!(settings.drain_timeout(), Duration::ZERO);
    assert_eq!(settings.cancel_grace(), Duration::ZERO);
}

#[test]
fn given_an_empty_runner_version_when_built_then_the_settings_are_built_as_the_chart_allows() {
    let settings = complete().runner_version("").build().expect("settings");
    assert_eq!(settings.runner_version(), "");
}
