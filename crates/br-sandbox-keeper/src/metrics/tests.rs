use super::*;

fn series(text: &str, name: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with(&format!("br_sandbox_keeper_{name}")))
        .map(str::to_owned)
        .collect()
}

#[test]
fn it_renders_every_metric_name_with_its_type() {
    let metrics = Metrics::new();
    metrics.set_pool_desired("p", 3);
    let text = metrics.render(false);
    for (name, kind) in [
        ("pool_desired", "gauge"),
        ("pool_warm_ready", "gauge"),
        ("claims_per_second", "gauge"),
        ("idle_pods", "gauge"),
        ("pods_deleted_total", "counter"),
        ("saves_deleted_total", "counter"),
        ("tombstones_written_total", "counter"),
        ("orphans_deleted_total", "counter"),
        ("sweep_errors_total", "counter"),
        ("unreadable_pods", "gauge"),
        ("is_leader", "gauge"),
    ] {
        assert!(
            text.contains(&format!("# TYPE br_sandbox_keeper_{name} {kind}\n")),
            "{name}"
        );
    }
}

#[test]
fn it_renders_no_other_metric() {
    let metrics = Metrics::new();
    metrics.set_pool_desired("p", 3);
    let text = metrics.render(false);
    let names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("# TYPE br_sandbox_keeper_"))
        .filter_map(|line| line.split(' ').next())
        .collect();
    assert_eq!(names.len(), 11, "{names:?}");
}

#[test]
fn the_three_pool_gauges_carry_the_label_pool_and_no_other_label() {
    let metrics = Metrics::new();
    metrics.set_pool_desired("p", 3);
    metrics.set_pool_warm_ready("p", 2);
    metrics.set_claims_per_second("p", 0.5);
    let text = metrics.render(false);
    assert_eq!(
        series(&text, "pool_desired"),
        vec!["br_sandbox_keeper_pool_desired{pool=\"p\"} 3"]
    );
    assert_eq!(
        series(&text, "pool_warm_ready"),
        vec!["br_sandbox_keeper_pool_warm_ready{pool=\"p\"} 2"]
    );
    assert_eq!(
        series(&text, "claims_per_second"),
        vec!["br_sandbox_keeper_claims_per_second{pool=\"p\"} 0.5"]
    );
}

#[test]
fn idle_pods_has_no_label() {
    let metrics = Metrics::new();
    metrics.set_pool_desired("p", 3);
    metrics.set_idle_pods(4);
    assert_eq!(
        series(&metrics.render(false), "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 4"]
    );
}

#[test]
fn the_metrics_with_no_label_render_one_series_from_the_start() {
    let text = Metrics::new().render(false);
    for name in [
        "idle_pods",
        "saves_deleted_total",
        "tombstones_written_total",
        "orphans_deleted_total",
        "sweep_errors_total",
        "unreadable_pods",
        "is_leader",
    ] {
        assert_eq!(
            series(&text, name),
            vec![format!("br_sandbox_keeper_{name} 0")],
            "{name}"
        );
    }
}

#[test]
fn pods_deleted_carries_the_label_reason_with_the_five_reasons_from_the_start() {
    let text = Metrics::new().render(false);
    let mut found = series(&text, "pods_deleted_total");
    found.sort();
    let mut expected: Vec<String> = ["warm_max_age", "idle", "stuck", "failed", "rollout"]
        .iter()
        .map(|reason| format!("br_sandbox_keeper_pods_deleted_total{{reason=\"{reason}\"}} 0"))
        .collect();
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn values_follow_the_calls() {
    let metrics = Metrics::new();
    metrics.set_pool_desired("p", 7);
    metrics.pod_deleted("idle");
    metrics.pod_deleted("idle");
    metrics.save_deleted();
    metrics.tombstone_written();
    metrics.orphan_deleted();
    metrics.sweep_error();
    metrics.set_unreadable_pods(4);
    metrics.set_idle_pods(5);
    let text = metrics.render(true);
    assert!(text.contains("br_sandbox_keeper_pool_desired{pool=\"p\"} 7"));
    assert!(text.contains("br_sandbox_keeper_pods_deleted_total{reason=\"idle\"} 2"));
    assert!(text.contains("br_sandbox_keeper_saves_deleted_total 1"));
    assert!(text.contains("br_sandbox_keeper_tombstones_written_total 1"));
    assert!(text.contains("br_sandbox_keeper_orphans_deleted_total 1"));
    assert!(text.contains("br_sandbox_keeper_sweep_errors_total 1"));
    assert!(text.contains("br_sandbox_keeper_unreadable_pods 4"));
    assert!(text.contains("br_sandbox_keeper_idle_pods 5"));
    assert!(text.contains("br_sandbox_keeper_is_leader 1"));
}

#[test]
fn it_escapes_label_values() {
    let metrics = Metrics::new();
    metrics.set_pool_desired("a\"b\\c\nd", 1);
    let text = metrics.render(false);
    assert!(text.contains("{pool=\"a\\\"b\\\\c\\nd\"} 1"));
}
