use br_runner_jobs::{
    CallMode, CallerHandle, Content, CreateChildInput, PayloadForm, Removal, SyncEnd,
};
use uuid::Uuid;

fn removal_text(removal: Removal) -> String {
    format!("a write was removed: {removal:?}")
}

pub async fn call_a_runner(
    caller: &CallerHandle,
    child: Uuid,
    runner_type: &str,
    request: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let payload = Content {
        message: request.to_owned(),
        attachments: Vec::new(),
    };
    let uploaded = caller.upload_payload(child, payload)?;
    let input = CreateChildInput::new(
        child,
        Uuid::now_v7(),
        runner_type,
        CallMode::Sync,
        PayloadForm::Kept,
    );
    let created = caller.create_child(input)?;
    let end = caller.end_wait(child);
    uploaded.await.map_err(removal_text)?;
    created.await.map_err(removal_text)?;
    let answer = match end.await? {
        SyncEnd::Completed {
            inline: Some(content),
            ..
        } => content.message,
        SyncEnd::Completed {
            result_ref: Some(result),
            ..
        } => caller.read_result(result, false).await?.message,
        SyncEnd::Completed { .. } => String::new(),
        SyncEnd::Failed { message, .. } | SyncEnd::Cancelled { message } => {
            return Err(message.into());
        }
        _ => return Err("the call ended in a way this example does not know".into()),
    };
    caller
        .finish(child, Uuid::now_v7())?
        .await
        .map_err(removal_text)?;
    Ok(answer)
}

fn main() {
    println!("call_a_runner(&caller, child_job_id, runner_type, request) needs a CallerHandle");
}
