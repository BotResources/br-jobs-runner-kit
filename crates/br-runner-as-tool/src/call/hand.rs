use br_agentic_contract::CancelView;
use std::future::Future;

use br_runner_jobs::{CreateChildInput, PayloadForm, WriteEnd, WriteResult};
use futures::FutureExt;
use uuid::Uuid;

use super::prepare::Prepared;
use super::{RunnerToolCode, Stop};

impl RunnerToolCode {
    pub(super) fn hand_upload(
        &self,
        child: Uuid,
        prepared: &mut Prepared,
        cancel: &CancelView,
    ) -> Result<(), Stop> {
        let Some(content) = prepared.upload.take() else {
            return Ok(());
        };
        if cancel.is_triggered() {
            return Err(Stop::Cancelled);
        }
        self.caller
            .upload_payload(child, content)
            .map(drop)
            .map_err(Stop::fault)
    }

    pub(super) fn hand_create(
        &self,
        child: Uuid,
        form: PayloadForm,
        cancel: &CancelView,
    ) -> Result<WriteEnd, Stop> {
        if cancel.is_triggered() {
            return Err(Stop::Cancelled);
        }
        let mut input = CreateChildInput::new(
            child,
            Uuid::now_v7(),
            self.runner_type.clone(),
            self.mode.call_mode(),
            form,
        );
        input.config = self.config.clone();
        input.max_attempts = self.max_attempts;
        self.caller.create_child(input).map_err(Stop::fault)
    }

    pub(super) fn hand_cancel(&self, child: Uuid) {
        let _ = self.caller.cancel(child, Uuid::now_v7());
    }

    pub(super) fn cancel_unless_dead<F>(&self, child: Uuid, created: &mut F)
    where
        F: Future<Output = WriteResult> + Unpin,
    {
        let dead = matches!(created.now_or_never(), Some(Err(_)));
        if !dead {
            self.hand_cancel(child);
        }
    }

    pub(super) fn hand_finish(&self, child: Uuid) {
        let _ = self.caller.finish(child, Uuid::now_v7());
    }
}
