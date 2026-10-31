use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_graph_contract::InstanceId;
use futures::FutureExt;

use crate::record::RunRecord;
use crate::shared::Shared;

pub(crate) fn call_once(shared: &Arc<Shared>, record: &Arc<RunRecord>, instance: InstanceId) {
    let Some(hook) = shared.hook.clone() else {
        return;
    };
    let Some(ended) = record.claim_hook(instance) else {
        return;
    };
    record.hook_started();
    let record = Arc::clone(record);
    tokio::spawn(async move {
        let run_id = record.run_id;
        let called = AssertUnwindSafe(async move { hook(ended).await })
            .catch_unwind()
            .await;
        if called.is_err() {
            tracing::error!(run_id = %run_id, "the lifecycle hook panicked");
        }
        record.hook_returned();
    });
}

pub(crate) fn call_all(shared: &Arc<Shared>, record: &Arc<RunRecord>) {
    for (instance, _) in record.unhooked_instances() {
        call_once(shared, record, instance);
    }
}
