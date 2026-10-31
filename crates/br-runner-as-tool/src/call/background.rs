use br_agentic_contract::{CancelView, ToolBlock, ToolContent};
use uuid::Uuid;

use super::prepare::Prepared;
use super::{RunnerToolCode, Stop};

impl RunnerToolCode {
    pub(super) fn run_background(
        &self,
        mut prepared: Prepared,
        cancel: &CancelView,
    ) -> Result<ToolContent, Stop> {
        let child = Uuid::now_v7();
        self.hand_upload(child, &mut prepared, cancel)?;
        drop(self.hand_create(child, prepared.form, cancel)?);
        if cancel.is_triggered() {
            self.hand_cancel(child);
            return Err(Stop::Cancelled);
        }
        let text = format!(
            "The runner {runner} was asked to do the work as the child Job {child}. \
             The create was handed over to jobs and is not confirmed until the notice \
             \"accepted\" reaches you. The notices of this work name the child Job {child}.",
            runner = self.runner_type
        );
        let block = ToolBlock::text(text).map_err(Stop::fault)?;
        Ok(ToolContent::new(vec![block], false))
    }
}
