use std::sync::LazyLock;

use contract_jobs::graphql_roots::{
    ATTACHMENT_ROOT, MAIL_ROOT, PAYLOAD_ROOT, READ_RESULT_ROOT, SAVE_STATE_ROOT, SAVED_STATE_ROOT,
    SEND_MAIL_ROOT, UNREAD_ROOT, UPLOAD_ATTACHMENT_ROOT, UPLOAD_PAYLOAD_ROOT, UPLOAD_RESULT_ROOT,
};

const OUTCOME: &str = "success reason message";
const CONTENT: &str = "message attachments { name contentType content }";
const ATTACHMENT_META: &str = "attachmentId name contentType size";

pub(super) static PAYLOAD: LazyLock<String> = LazyLock::new(|| {
    format!(
        "query($work: JobsWorkInput!) {{ {PAYLOAD_ROOT}(work: $work) {{ {OUTCOME} content {{ {CONTENT} }} }} }}"
    )
});

pub(super) static UPLOAD_PAYLOAD: LazyLock<String> = LazyLock::new(|| {
    format!(
        "mutation($input: JobsUploadPayloadInput!) {{ {UPLOAD_PAYLOAD_ROOT}(input: $input) {{ {OUTCOME} }} }}"
    )
});

pub(super) static UPLOAD_RESULT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "mutation($input: JobsUploadResultInput!) {{ {UPLOAD_RESULT_ROOT}(input: $input) {{ {OUTCOME} }} }}"
    )
});

pub(super) static READ_RESULT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "mutation($input: JobsReadResultInput!) {{ {READ_RESULT_ROOT}(input: $input) {{ {OUTCOME} content {{ {CONTENT} }} }} }}"
    )
});

pub(super) static UPLOAD_ATTACHMENT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "mutation($input: JobsUploadAttachmentInput!) {{ {UPLOAD_ATTACHMENT_ROOT}(input: $input) {{ {OUTCOME} }} }}"
    )
});

pub(super) static ATTACHMENT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "query($work: JobsWorkInput!, $mailId: UUID!, $attachmentId: UUID!) {{ {ATTACHMENT_ROOT}(work: $work, mailId: $mailId, attachmentId: $attachmentId) {{ {OUTCOME} attachment {{ name contentType content }} }} }}"
    )
});

pub(super) static SEND_MAIL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "mutation($input: JobsSendMailInput!) {{ {SEND_MAIL_ROOT}(input: $input) {{ {OUTCOME} badAddresses {{ address reason message }} restartRefused {{ address reason message }} }} }}"
    )
});

pub(super) static UNREAD: LazyLock<String> = LazyLock::new(|| {
    format!(
        "query($work: JobsWorkInput!, $address: UUID) {{ {UNREAD_ROOT}(work: $work, address: $address) {{ {OUTCOME} items {{ itemId seq itemKind address runId mailId subject from author sourceConversationId body attachments {{ {ATTACHMENT_META} }} noticeType jobId position message resultId failureReport answer }} }} }}"
    )
});

pub(super) static MAIL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "query($work: JobsWorkInput!, $mailId: UUID!, $address: UUID!) {{ {MAIL_ROOT}(work: $work, mailId: $mailId, address: $address) {{ {OUTCOME} mail {{ mailId from author sourceConversationId subject body to cc attachments {{ {ATTACHMENT_META} }} }} }} }}"
    )
});

pub(super) static SAVE_STATE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "mutation($input: JobsSaveStateInput!) {{ {SAVE_STATE_ROOT}(input: $input) {{ {OUTCOME} }} }}"
    )
});

pub(super) static SAVED_STATE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "query($work: JobsWorkInput!) {{ {SAVED_STATE_ROOT}(work: $work) {{ {OUTCOME} state agentIds mailAddresses }} }}"
    )
});
