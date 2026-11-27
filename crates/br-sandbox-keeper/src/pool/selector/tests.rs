use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelectorRequirement;

use super::*;

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn selects(selector: &str, pairs: &[(&str, &str)]) -> bool {
    Selector::parse(selector).unwrap().matches(&labels(pairs))
}

#[test]
fn the_default_selector_of_the_keeper_matches_a_label_with_that_value_only() {
    let selector = "br.sandbox/managed=true";
    assert!(selects(selector, &[("br.sandbox/managed", "true")]));
    assert!(!selects(selector, &[("br.sandbox/managed", "false")]));
    assert!(!selects(selector, &[]));
    assert!(selects(
        "br.sandbox/managed==true",
        &[("br.sandbox/managed", "true")]
    ));
}

#[test]
fn a_not_equal_requirement_matches_a_namespace_that_has_no_such_label() {
    assert!(selects("team!=a", &[]));
    assert!(selects("team!=a", &[("team", "b")]));
    assert!(!selects("team!=a", &[("team", "a")]));
}

#[test]
fn the_set_requirements_in_and_notin_follow_the_rule_of_kubernetes() {
    assert!(selects("env in (prod, staging)", &[("env", "staging")]));
    assert!(!selects("env in (prod,staging)", &[("env", "dev")]));
    assert!(!selects("env in (prod)", &[]));
    assert!(selects("env notin (prod)", &[("env", "dev")]));
    assert!(selects("env notin (prod)", &[]));
    assert!(!selects("env notin (prod, dev)", &[("env", "dev")]));
}

#[test]
fn exists_and_absent_look_at_the_key_only() {
    assert!(selects("managed", &[("managed", "")]));
    assert!(!selects("managed", &[]));
    assert!(selects("!managed", &[]));
    assert!(!selects("!managed", &[("managed", "x")]));
}

#[test]
fn every_requirement_must_hold_and_a_comma_inside_a_list_does_not_split() {
    let selector = "a=1,b in (x,y),!c";
    assert!(selects(selector, &[("a", "1"), ("b", "y")]));
    assert!(!selects(selector, &[("a", "1"), ("b", "y"), ("c", "1")]));
    assert!(!selects(selector, &[("a", "2"), ("b", "y")]));
}

#[test]
fn an_empty_selector_matches_every_namespace() {
    assert!(selects("", &[]));
    assert!(selects("  ", &[("a", "b")]));
}

#[test]
fn a_selector_that_cannot_be_read_is_an_error_and_never_a_match() {
    for text in [
        "=b",
        "a b",
        "a in b",
        "a in (x",
        "a,,b",
        "a>3",
        "a=b,",
        "a in (x))",
    ] {
        assert!(Selector::parse(text).is_err(), "{text}");
    }
}

#[test]
fn a_label_selector_object_is_read_with_its_labels_and_its_expressions() {
    let selector = LabelSelector {
        match_labels: Some(labels(&[("a", "1")])),
        match_expressions: Some(vec![
            LabelSelectorRequirement {
                key: "b".to_owned(),
                operator: "In".to_owned(),
                values: Some(vec!["x".to_owned()]),
            },
            LabelSelectorRequirement {
                key: "c".to_owned(),
                operator: "DoesNotExist".to_owned(),
                values: None,
            },
        ]),
    };
    let parsed = Selector::from_label_selector(&selector).unwrap();
    assert!(parsed.matches(&labels(&[("a", "1"), ("b", "x")])));
    assert!(!parsed.matches(&labels(&[("a", "1"), ("b", "x"), ("c", "")])));
    assert!(!parsed.matches(&labels(&[("a", "2"), ("b", "x")])));
}

#[test]
fn an_empty_label_selector_object_matches_everything_and_an_unknown_operator_is_an_error() {
    let empty = Selector::from_label_selector(&LabelSelector::default()).unwrap();
    assert!(empty.matches(&labels(&[])));
    let bad = LabelSelector {
        match_labels: None,
        match_expressions: Some(vec![LabelSelectorRequirement {
            key: "a".to_owned(),
            operator: "Gt".to_owned(),
            values: None,
        }]),
    };
    assert!(Selector::from_label_selector(&bad).is_err());
}
