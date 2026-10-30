use br_agentic_contract::ThreadRef;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::error::Error;
use crate::ids::require_v7;

#[derive(Debug, Clone)]
pub struct SetTitle {
    pub thread: ThreadRef,
    pub title_job_id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct FailTitleJob {
    pub thread: ThreadRef,
    pub title_job_id: Uuid,
    pub code: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadRequest {
    pub artifact_id: Uuid,
    pub thread: ThreadRef,
    pub name: String,
    pub media_type: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UploadAccess {
    pub url: String,
    pub fields: Map<String, Value>,
}

pub(crate) const CONTEXT: &str = "query($threadId: UUID!, $agentId: String) { threadsContext(threadId: $threadId, agentId: $agentId) { threadId conversation projectedThread members { userId displayName inRoster } resources { key ids } } }";
const SET_TITLE: &str = "mutation($threadId: UUID!, $jobId: UUID!, $title: String!) { threadsSetTitle(threadId: $threadId, jobId: $jobId, title: $title) { success } }";
const FAIL_TITLE: &str = "mutation($threadId: UUID!, $jobId: UUID!, $reasonCode: String!, $message: String) { threadsFailTitleJob(threadId: $threadId, jobId: $jobId, reasonCode: $reasonCode, message: $message) { success } }";
const UPLOAD: &str = "mutation($threadId: UUID!, $artifactId: UUID!, $name: String!, $mediaType: String!, $size: Int!, $sha256: String!) { threadsRequestArtifactUpload(threadId: $threadId, artifactId: $artifactId, name: $name, mediaType: $mediaType, size: $size, sha256: $sha256) { url fields } }";

pub(crate) fn body(query: &str, variables: Value) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(&json!({ "query": query, "variables": variables }))
        .map_err(|e| Error::invalid(e.to_string()))
}

pub(crate) fn set_title(title: &SetTitle) -> Result<Vec<u8>, Error> {
    body(
        SET_TITLE,
        json!({
            "threadId": title.thread.as_uuid(),
            "jobId": title.title_job_id,
            "title": title.text,
        }),
    )
}

pub(crate) fn fail_title(fail: &FailTitleJob) -> Result<Vec<u8>, Error> {
    body(
        FAIL_TITLE,
        json!({
            "threadId": fail.thread.as_uuid(),
            "jobId": fail.title_job_id,
            "reasonCode": fail.code,
            "message": fail.message,
        }),
    )
}

pub(crate) fn upload(request: &UploadRequest) -> Result<Vec<u8>, Error> {
    require_v7(request.artifact_id, "an artifact id")?;
    body(
        UPLOAD,
        json!({
            "threadId": request.thread.as_uuid(),
            "artifactId": request.artifact_id,
            "name": request.name,
            "mediaType": request.media_type,
            "size": request.size,
            "sha256": request.sha256,
        }),
    )
}

pub(crate) fn acknowledged(data: &Value, field: &str) -> Result<(), Error> {
    let success = data
        .get(field)
        .and_then(|ack| ack.get("success"))
        .and_then(Value::as_bool);
    match success {
        Some(true) => Ok(()),
        _ => Err(Error::Unreadable(format!("{field} was not acknowledged"))),
    }
}

pub(crate) fn upload_access(data: &Value) -> Result<UploadAccess, Error> {
    let access = data
        .get("threadsRequestArtifactUpload")
        .ok_or_else(|| Error::Unreadable("the upload access is missing".to_owned()))?;
    serde_json::from_value(access.clone())
        .map_err(|e| Error::Unreadable(format!("the upload access is malformed: {e}")))
}
