use br_core_auth::Passport;
use contract_jobs::command_v3::{CallMode as WireMode, Payload};
use contract_jobs::runner::TriggerV2;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::error::Error;
use crate::model::{CallMode, Content, ContentAttachment, PayloadForm, Restart};

const KNOWN: [&str; 13] = [
    "version",
    "run_id",
    "job_id",
    "attempt",
    "parent_job_id",
    "initial_job_id",
    "passport",
    "triggered_by",
    "config",
    "payload",
    "mode",
    "caller_address",
    "restart",
];

pub(crate) struct Parsed {
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub attempt: u32,
    pub parent_job_id: Uuid,
    pub initial_job_id: Uuid,
    pub passport: Passport,
    pub passport_json: Value,
    pub triggered_by: Option<Uuid>,
    pub config: Option<Value>,
    pub payload: Option<PayloadForm>,
    pub call: Option<CallMode>,
    pub restart: Option<Restart>,
    pub unknown: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unreadable {
    pub error: Error,
    pub job_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
}

fn lenient_id(value: &Value, key: &str) -> Option<Uuid> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|text| Uuid::parse_str(text).ok())
}

fn payload_form(payload: Payload) -> PayloadForm {
    match payload {
        Payload::Inline {
            message,
            attachments,
        } => PayloadForm::Inline(Content {
            message,
            attachments: attachments
                .into_iter()
                .map(|a| ContentAttachment {
                    name: a.name,
                    content_type: a.content_type,
                    content: a.content,
                })
                .collect(),
        }),
        Payload::Kept => PayloadForm::Kept,
        Payload::Reference {
            service,
            document_id,
        } => PayloadForm::Reference {
            service,
            document_id,
        },
    }
}

fn call_mode(trigger: &TriggerV2) -> Result<Option<CallMode>, String> {
    match (trigger.mode, trigger.caller_address) {
        (None, _) => Ok(None),
        (Some(WireMode::Sync), _) => Ok(Some(CallMode::Sync)),
        (Some(WireMode::Background), Some(caller_address)) => {
            Ok(Some(CallMode::Background { caller_address }))
        }
        (Some(WireMode::Background), None) => {
            Err("a background call with no caller address".to_owned())
        }
    }
}

pub(crate) fn trigger(value: Value) -> Result<Parsed, Unreadable> {
    let job_id = lenient_id(&value, "job_id");
    let run_id = lenient_id(&value, "run_id");
    let failed = |error: Error| Unreadable {
        error,
        job_id,
        run_id,
    };
    let unreadable = |text: String| failed(Error::Unreadable(text));
    let Some(object) = value.as_object() else {
        return Err(unreadable("the trigger is not an object".to_owned()));
    };
    let unknown: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let wire: TriggerV2 = serde_json::from_value(value)
        .map_err(|e| failed(Error::unreadable_json("the trigger", &e)))?;
    let passport: Passport = serde_json::from_value(wire.passport.clone())
        .map_err(|_| unreadable("the Passport of the trigger cannot be read".to_owned()))?;
    let call = call_mode(&wire).map_err(unreadable)?;
    Ok(Parsed {
        job_id: wire.job_id,
        run_id: wire.run_id,
        attempt: wire.attempt,
        parent_job_id: wire.parent_job_id,
        initial_job_id: wire.initial_job_id,
        passport,
        passport_json: wire.passport,
        triggered_by: wire.triggered_by,
        config: wire.config,
        payload: wire.payload.map(payload_form),
        call,
        restart: wire.restart.map(|r| Restart {
            restart_of_run_id: r.restart_of_run_id,
        }),
        unknown,
    })
}
