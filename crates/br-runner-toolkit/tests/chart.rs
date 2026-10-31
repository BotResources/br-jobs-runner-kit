use std::collections::BTreeSet;
use std::time::Duration;

use br_runner_toolkit::{Settings, SettingsBuilder};

const SCHEMA: &str = include_str!("../../../charts/br-runner-toolkit-chart/values.schema.json");
const TEMPLATE: &str = include_str!("../../../charts/br-runner-toolkit-chart/templates/_env.tpl");
const README: &str = include_str!("../../../charts/br-runner-toolkit-chart/README.md");

fn schema() -> serde_json::Value {
    serde_json::from_str(SCHEMA).expect("the schema is JSON")
}

fn required_names() -> Vec<String> {
    schema()["required"]
        .as_array()
        .expect("a required list")
        .iter()
        .map(|name| name.as_str().expect("a name").to_owned())
        .collect()
}

fn set(builder: SettingsBuilder, name: &str) -> SettingsBuilder {
    let wait = Duration::from_millis(1000);
    match name {
        "runner_version" => builder.runner_version("1.0.0"),
        "max_concurrent_runs" => builder.max_concurrent_runs(1),
        "ack_wait" => builder.ack_wait(wait),
        "drain_timeout" => builder.drain_timeout(wait),
        "cancel_grace" => builder.cancel_grace(wait),
        "claim_wait" => builder.claim_wait(wait),
        "outcome_wait" => builder.outcome_wait(wait),
        "retry_wait" => builder.retry_wait(wait),
        other => panic!("the chart has a value that is not a setting: {other}"),
    }
}

fn settings_from(names: &[String]) -> Result<Settings, br_runner_toolkit::Error> {
    names
        .iter()
        .fold(Settings::builder(), |builder, name| set(builder, name))
        .build()
}

#[test]
fn the_values_of_the_chart_are_exactly_the_settings_of_the_library() {
    let names = required_names();
    assert_eq!(names.len(), 8);
    settings_from(&names).expect("every setting has a value in the chart");
}

#[test]
fn a_value_of_the_chart_is_a_setting_and_a_setting_is_a_value_of_the_chart() {
    for name in required_names() {
        let others: Vec<String> = required_names()
            .into_iter()
            .filter(|other| *other != name)
            .collect();
        let error = settings_from(&others)
            .err()
            .unwrap_or_else(|| panic!("{name} is not needed by the settings"));
        assert_eq!(error.code(), br_runner_toolkit::ErrorCode::SettingMissing);
    }
}

#[test]
fn the_properties_of_the_schema_are_the_required_values_and_the_global_values_of_helm() {
    let schema = schema();
    let properties: BTreeSet<String> = schema["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .filter(|name| *name != "global")
        .cloned()
        .collect();
    let required: BTreeSet<String> = required_names().into_iter().collect();
    assert_eq!(properties, required);
}

#[test]
fn the_template_and_the_readme_name_every_value() {
    for name in required_names() {
        assert!(TEMPLATE.contains(&format!("$runner.{name}")), "{name}");
        assert!(README.contains(&format!("`{name}`")), "{name}");
    }
}

#[test]
#[should_panic(expected = "not a setting")]
fn a_value_that_is_not_a_setting_is_refused_by_the_check() {
    set(Settings::builder(), "unknown_value");
}
