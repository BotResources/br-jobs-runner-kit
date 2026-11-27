use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;

pub const POD_DELETE_REASONS: [&str; 5] = ["warm_max_age", "idle", "stuck", "failed", "rollout"];

#[derive(Default)]
struct PoolGauges {
    desired: u32,
    warm_ready: u32,
    claims_per_second: f64,
}

#[derive(Default)]
struct State {
    pools: BTreeMap<String, PoolGauges>,
    pods_deleted: BTreeMap<String, u64>,
    saves_deleted: u64,
    tombstones_written: u64,
    orphans_deleted: u64,
    sweep_errors: u64,
    unreadable_pods: u64,
    idle_pods: u64,
}

#[derive(Default)]
pub struct Metrics {
    state: Mutex<State>,
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP br_sandbox_keeper_{name} {help}");
    let _ = writeln!(out, "# TYPE br_sandbox_keeper_{name} {kind}");
}

fn pool_series<T: std::fmt::Display>(
    out: &mut String,
    name: &str,
    help: &str,
    pools: &BTreeMap<String, PoolGauges>,
    value: impl Fn(&PoolGauges) -> T,
) {
    header(out, name, "gauge", help);
    for (pool, gauges) in pools {
        let _ = writeln!(
            out,
            "br_sandbox_keeper_{name}{{pool=\"{}\"}} {}",
            escape(pool),
            value(gauges)
        );
    }
}

fn plain(out: &mut String, name: &str, kind: &str, help: &str, value: u64) {
    header(out, name, kind, help);
    let _ = writeln!(out, "br_sandbox_keeper_{name} {value}");
}

impl Metrics {
    pub fn new() -> Self {
        let metrics = Self::default();
        {
            let mut state = metrics.lock();
            for reason in POD_DELETE_REASONS {
                state.pods_deleted.insert(reason.to_owned(), 0);
            }
        }
        metrics
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_pool(&self, pool: &str, change: impl FnOnce(&mut PoolGauges)) {
        change(self.lock().pools.entry(pool.to_owned()).or_default());
    }

    pub fn set_pool_desired(&self, pool: &str, desired: u32) {
        self.with_pool(pool, |gauges| gauges.desired = desired);
    }

    pub fn set_pool_warm_ready(&self, pool: &str, warm_ready: u32) {
        self.with_pool(pool, |gauges| gauges.warm_ready = warm_ready);
    }

    pub fn set_claims_per_second(&self, pool: &str, rate: f64) {
        self.with_pool(pool, |gauges| gauges.claims_per_second = rate);
    }

    pub fn set_idle_pods(&self, count: usize) {
        self.lock().idle_pods = u64::try_from(count).unwrap_or(u64::MAX);
    }

    pub fn pod_deleted(&self, reason: &str) {
        let mut state = self.lock();
        let count = state.pods_deleted.entry(reason.to_owned()).or_default();
        *count = count.saturating_add(1);
    }

    pub fn save_deleted(&self) {
        let mut state = self.lock();
        state.saves_deleted = state.saves_deleted.saturating_add(1);
    }

    pub fn tombstone_written(&self) {
        let mut state = self.lock();
        state.tombstones_written = state.tombstones_written.saturating_add(1);
    }

    pub fn orphan_deleted(&self) {
        let mut state = self.lock();
        state.orphans_deleted = state.orphans_deleted.saturating_add(1);
    }

    pub fn sweep_error(&self) {
        let mut state = self.lock();
        state.sweep_errors = state.sweep_errors.saturating_add(1);
    }

    pub fn set_unreadable_pods(&self, count: usize) {
        self.lock().unreadable_pods = u64::try_from(count).unwrap_or(u64::MAX);
    }

    pub fn render(&self, is_leader: bool) -> String {
        let state = self.lock();
        let mut out = String::new();
        pool_series(
            &mut out,
            "pool_desired",
            "Replicas the keeper wants for the pool.",
            &state.pools,
            |g| g.desired,
        );
        pool_series(
            &mut out,
            "pool_warm_ready",
            "Warm pods that are ready.",
            &state.pools,
            |g| g.warm_ready,
        );
        pool_series(
            &mut out,
            "claims_per_second",
            "Peak claim rate seen in the window.",
            &state.pools,
            |g| g.claims_per_second,
        );
        plain(
            &mut out,
            "idle_pods",
            "gauge",
            "Claimed pods with a saved state and no running turn.",
            state.idle_pods,
        );
        header(
            &mut out,
            "pods_deleted_total",
            "counter",
            "Pods deleted, by reason.",
        );
        for (reason, count) in &state.pods_deleted {
            let _ = writeln!(
                out,
                "br_sandbox_keeper_pods_deleted_total{{reason=\"{}\"}} {count}",
                escape(reason)
            );
        }
        plain(
            &mut out,
            "saves_deleted_total",
            "counter",
            "Saves deleted.",
            state.saves_deleted,
        );
        plain(
            &mut out,
            "tombstones_written_total",
            "counter",
            "Tombstones written.",
            state.tombstones_written,
        );
        plain(
            &mut out,
            "orphans_deleted_total",
            "counter",
            "Orphan objects deleted.",
            state.orphans_deleted,
        );
        plain(
            &mut out,
            "sweep_errors_total",
            "counter",
            "Actions that failed after the start.",
            state.sweep_errors,
        );
        plain(
            &mut out,
            "unreadable_pods",
            "gauge",
            "Pods whose facts cannot be read.",
            state.unreadable_pods,
        );
        plain(
            &mut out,
            "is_leader",
            "gauge",
            "1 while this replica holds the lease.",
            u64::from(is_leader),
        );
        out
    }
}

#[cfg(test)]
mod tests;
