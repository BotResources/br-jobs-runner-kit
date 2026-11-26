use std::time::Duration;

use br_agentic_harness_contract::{OpenFault, PortError};
use br_runner_sandbox::{
    ConfigError, Error, FormatError, ImageError, SeedError, WebAccessMismatch,
};

#[test]
fn the_image_errors_are_typed_causes_of_a_port_fault_that_the_accessor_tells_apart() {
    let unavailable = ImageError::unavailable(Duration::from_secs(30)).port_error();
    let failing = ImageError::failing("ImagePullFailing", "back-off pulling image").port_error();
    assert!(!unavailable.is_refusal());
    assert!(!failing.is_refusal());
    assert_eq!(
        OpenFault::of(&unavailable),
        Some(OpenFault::ImageUnavailable)
    );
    assert_eq!(OpenFault::of(&failing), Some(OpenFault::ImageFailing));
}

#[test]
fn the_image_errors_keep_their_details() {
    let unavailable = ImageError::unavailable(Duration::from_secs(30));
    assert!(unavailable.to_string().contains("30s"));
    let failing = ImageError::failing("StartCrashing", "the container exits with 1");
    assert!(failing.to_string().contains("StartCrashing"));
    assert!(failing.to_string().contains("the container exits with 1"));
    assert_eq!(failing.fault(), OpenFault::ImageFailing);
}

#[test]
fn the_crate_error_gives_the_same_causes_through_the_port_error() {
    let unavailable = PortError::from(Error::from(ImageError::unavailable(Duration::from_secs(1))));
    let failing = PortError::from(Error::from(ImageError::failing("r", "m")));
    assert_eq!(
        OpenFault::of(&unavailable),
        Some(OpenFault::ImageUnavailable)
    );
    assert_eq!(OpenFault::of(&failing), Some(OpenFault::ImageFailing));
}

#[test]
fn a_web_access_mismatch_and_a_refused_seed_path_are_port_refusals() {
    assert!(PortError::from(Error::from(WebAccessMismatch)).is_refusal());
    let seed = SeedError::Path {
        path: "../x".to_owned(),
    };
    assert!(PortError::from(Error::from(seed)).is_refusal());
}

#[test]
fn a_configuration_a_format_and_an_archive_failure_are_port_faults_with_no_open_cause() {
    let errors = [
        Error::from(ConfigError::ClaimAttempts),
        Error::from(FormatError::Level(0)),
        Error::from(SeedError::Archive("io".to_owned())),
    ];
    for error in errors {
        let port = PortError::from(error);
        assert!(!port.is_refusal());
        assert_eq!(OpenFault::of(&port), None);
    }
}

#[test]
fn a_port_refusal_with_another_reason_is_not_an_image_error() {
    assert_eq!(OpenFault::of(&PortError::refused("no")), None);
}
