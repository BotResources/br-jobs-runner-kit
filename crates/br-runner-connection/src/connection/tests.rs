use http::header::AUTHORIZATION;

use super::bearer;
use crate::endpoint::Endpoint;
use crate::http::{Gateway, default_config};

const SECRET: &str = "tok-0123456789-SECRET-abcdef";

#[test]
fn the_bearer_header_is_sensitive_and_no_debug_form_of_the_built_request_shows_the_token() {
    let value = bearer(SECRET).expect("bearer");
    assert!(value.is_sensitive());
    assert!(!format!("{value:?}").contains(SECRET));
    let endpoint = Endpoint::parse("https://gateway.example.test/graphql").expect("endpoint");
    let request = Gateway::new(endpoint, default_config())
        .request(value, b"{}".to_vec())
        .expect("request");
    assert!(request.headers()[AUTHORIZATION].is_sensitive());
    let shown = format!("{request:?} {:#?}", request.headers());
    assert!(!shown.contains(SECRET), "{shown}");
}
