use crate::common::{config, input, required};

const SECRETS: [&str; 2] = ["AKIDEXAMPLE", "wJalrSECRETEXAMPLE"];

fn shows_no_secret(text: &str) {
    for secret in SECRETS {
        assert!(!text.contains(secret), "{text} shows {secret}");
    }
}

#[test]
fn debug_and_display_of_the_configuration_show_no_part_of_the_credentials() {
    let config = config();
    shows_no_secret(&format!("{config:?}"));
    shows_no_secret(&format!("{config:#?}"));
    shows_no_secret(&config.to_string());
}

#[test]
fn debug_of_the_input_and_of_the_credentials_show_no_part_of_them() {
    let input = input(required()).expect("input");
    shows_no_secret(&format!("{input:?}"));
    shows_no_secret(&format!("{:?}", input.store_credentials));
    shows_no_secret(&input.store_credentials.to_string());
}

#[test]
fn the_credentials_stay_readable_for_the_store_client() {
    let config = config();
    let credentials = config.store().credentials();
    assert_eq!(credentials.access_key_id(), SECRETS[0]);
    assert_eq!(credentials.secret_access_key(), SECRETS[1]);
}
