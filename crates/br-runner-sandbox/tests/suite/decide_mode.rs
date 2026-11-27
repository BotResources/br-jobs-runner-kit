use br_agentic_harness_contract::{PortError, Site, WebAccess};
use br_runner_sandbox::{NetworkRule, WebAccessMismatch, check_web_access};
use serde_json::json;

use crate::common::{build, with};

fn site(text: &str) -> Site {
    Site::new(text).expect("site")
}

fn rule(extra: serde_json::Value) -> NetworkRule {
    build(with(extra)).expect("config").network().clone()
}

fn allow() -> NetworkRule {
    rule(json!({"network_mode": "allow", "allowlist": ["a.example.test", "b.example.test"]}))
}

#[test]
fn the_mode_none_accepts_an_absent_setting_none_and_a_whitelist_that_reaches_nothing() {
    let none = rule(json!({"network_mode": "none"}));
    assert_eq!(check_web_access(&none, None), Ok(()));
    assert_eq!(check_web_access(&none, Some(&WebAccess::None)), Ok(()));
    assert_eq!(
        check_web_access(&none, Some(&WebAccess::Whitelist(vec![]))),
        Ok(())
    );
}

#[test]
fn the_mode_none_refuses_full_and_a_whitelist_that_reaches_a_site() {
    let none = rule(json!({"network_mode": "none"}));
    assert_eq!(
        check_web_access(&none, Some(&WebAccess::Full)),
        Err(WebAccessMismatch)
    );
    assert_eq!(
        check_web_access(
            &none,
            Some(&WebAccess::Whitelist(vec![site("a.example.test")]))
        ),
        Err(WebAccessMismatch)
    );
}

#[test]
fn the_mode_full_accepts_full_and_nothing_else() {
    let full = rule(json!({"network_mode": "full"}));
    assert_eq!(check_web_access(&full, Some(&WebAccess::Full)), Ok(()));
    assert_eq!(check_web_access(&full, None), Err(WebAccessMismatch));
    assert_eq!(
        check_web_access(&full, Some(&WebAccess::None)),
        Err(WebAccessMismatch)
    );
    assert_eq!(
        check_web_access(
            &full,
            Some(&WebAccess::Whitelist(vec![site("a.example.test")]))
        ),
        Err(WebAccessMismatch)
    );
}

#[test]
fn the_mode_allow_accepts_the_same_sites_in_any_order_with_duplicates_ignored() {
    let allow = allow();
    for sites in [
        vec!["a.example.test", "b.example.test"],
        vec!["b.example.test", "a.example.test"],
        vec!["a.example.test", "b.example.test", "a.example.test"],
    ] {
        let access = WebAccess::Whitelist(sites.into_iter().map(site).collect());
        assert_eq!(check_web_access(&allow, Some(&access)), Ok(()));
    }
}

#[test]
fn the_mode_allow_refuses_a_subset_a_superset_another_site_and_every_other_setting() {
    let allow = allow();
    for sites in [
        vec!["a.example.test"],
        vec!["a.example.test", "b.example.test", "c.example.test"],
        vec!["a.example.test", "c.example.test"],
        vec![],
    ] {
        let access = WebAccess::Whitelist(sites.into_iter().map(site).collect());
        assert_eq!(
            check_web_access(&allow, Some(&access)),
            Err(WebAccessMismatch)
        );
    }
    assert_eq!(check_web_access(&allow, None), Err(WebAccessMismatch));
    assert_eq!(
        check_web_access(&allow, Some(&WebAccess::None)),
        Err(WebAccessMismatch)
    );
    assert_eq!(
        check_web_access(&allow, Some(&WebAccess::Full)),
        Err(WebAccessMismatch)
    );
}

#[test]
fn a_mismatch_is_a_port_refusal_with_a_reason_that_names_no_site() {
    let error = PortError::from(WebAccessMismatch);
    assert!(error.is_refusal());
    let reason = error.reason().expect("reason").to_string();
    assert!(!reason.contains("example.test"));
}
