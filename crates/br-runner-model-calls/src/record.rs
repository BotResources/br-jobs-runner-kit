use std::collections::BTreeMap;
use std::time::Duration;

use br_llm_messages::{ModelName, Usage};
use chrono::{DateTime, Utc};
use serde_json::{Map, Number, Value};
use uuid::Uuid;

use crate::CallEndpoint;
use crate::context::ModelCallContext;

/// What the library knows about one model call. The host reads it in
/// [`crate::RecordCommand::command`] and maps it to the payload of its contract.
///
/// It is not stored data: it has no serialization.
#[derive(Debug, Clone)]
pub struct ModelCallRecord {
    call_id: Uuid,
    context: ModelCallContext,
    endpoint: CallEndpoint,
    requested_model: ModelName,
    answered_model: Option<ModelName>,
    raw_usage: Option<Map<String, Value>>,
    usage: Option<Usage>,
    service_tier: Option<String>,
    inference_geo: Option<String>,
    speed: Option<String>,
    provider_cost: Option<Number>,
    started_at: DateTime<Utc>,
    duration: Duration,
    time_to_first_token: Option<Duration>,
    success: bool,
    abandoned: bool,
}

/// The parts of a record that the end of a call gives.
pub(crate) struct Ending {
    pub(crate) answered_model: Option<ModelName>,
    pub(crate) raw_usage: Option<Map<String, Value>>,
    pub(crate) usage: Option<Usage>,
    pub(crate) duration: Duration,
    pub(crate) time_to_first_token: Option<Duration>,
    pub(crate) success: bool,
    pub(crate) abandoned: bool,
}

/// The parts of a record known when the call starts.
pub(crate) struct Start {
    pub(crate) call_id: Uuid,
    pub(crate) context: ModelCallContext,
    pub(crate) endpoint: CallEndpoint,
    pub(crate) requested_model: ModelName,
    pub(crate) started_at: DateTime<Utc>,
}

fn text_member(raw: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    raw?.get(key)?.as_str().map(str::to_owned)
}

fn number_member(raw: Option<&Map<String, Value>>, key: &str) -> Option<Number> {
    match raw?.get(key)? {
        Value::Number(number) => Some(number.clone()),
        _ => None,
    }
}

impl ModelCallRecord {
    pub(crate) fn build(start: Start, ending: Ending) -> Self {
        let raw = ending.raw_usage.as_ref();
        let service_tier = text_member(raw, "service_tier");
        let inference_geo = text_member(raw, "inference_geo");
        let speed = text_member(raw, "speed");
        let provider_cost = number_member(raw, "cost");
        Self {
            call_id: start.call_id,
            context: start.context,
            endpoint: start.endpoint,
            requested_model: start.requested_model,
            answered_model: ending.answered_model,
            raw_usage: ending.raw_usage,
            usage: ending.usage,
            service_tier,
            inference_geo,
            speed,
            provider_cost,
            started_at: start.started_at,
            duration: ending.duration,
            time_to_first_token: ending.time_to_first_token,
            success: ending.success && !ending.abandoned,
            abandoned: ending.abandoned,
        }
    }

    /// The UUIDv7 the envelope minted for the call. The host sets the command id
    /// to it; the library publishes with it as the message id of the bus.
    pub fn call_id(&self) -> Uuid {
        self.call_id
    }

    pub fn job_id(&self) -> Uuid {
        self.context.job_id()
    }

    pub fn run_id(&self) -> Uuid {
        self.context.run_id()
    }

    pub fn attempt(&self) -> u32 {
        self.context.attempt()
    }

    pub fn initial_job_id(&self) -> Uuid {
        self.context.initial_job_id()
    }

    /// The user the work runs for, absent for work that runs for no user.
    pub fn user_id(&self) -> Option<Uuid> {
        self.context.user_id()
    }

    /// The labels of the context, ordered by key.
    pub fn labels(&self) -> &BTreeMap<String, String> {
        self.context.labels()
    }

    /// The endpoint of the wrapped client: provider, dialect, region, base URL.
    pub fn endpoint(&self) -> &CallEndpoint {
        &self.endpoint
    }

    /// The model of the settings of the wrapped client.
    pub fn requested_model(&self) -> &ModelName {
        &self.requested_model
    }

    /// The model of the call report, or for an abandoned call the last model the
    /// deltas carried; absent when the provider named none.
    pub fn answered_model(&self) -> Option<&ModelName> {
        self.answered_model.as_ref()
    }

    /// The raw usage block of the provider as the client kept it, absent when the
    /// call kept none.
    pub fn raw_usage(&self) -> Option<&Map<String, Value>> {
        self.raw_usage.as_ref()
    }

    /// The common usage of the call report, or for an abandoned call the last
    /// usage the deltas carried.
    pub fn usage(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }

    /// The top-level text member `service_tier` of the raw usage.
    pub fn service_tier(&self) -> Option<&str> {
        self.service_tier.as_deref()
    }

    /// The top-level text member `inference_geo` of the raw usage.
    pub fn inference_geo(&self) -> Option<&str> {
        self.inference_geo.as_deref()
    }

    /// The top-level text member `speed` of the raw usage.
    pub fn speed(&self) -> Option<&str> {
        self.speed.as_deref()
    }

    /// The top-level numeric member `cost` of the raw usage, as the provider
    /// reported it. The library does not use it.
    pub fn provider_cost(&self) -> Option<&Number> {
        self.provider_cost.as_ref()
    }

    /// The start of the call on the wall clock, in UTC.
    pub fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    /// The total duration of the call report, or for an abandoned call the time
    /// from the start to the drop on the monotonic clock.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// The time to the first event of the call report, or for an abandoned call
    /// the time to the first delta; absent when none came.
    pub fn time_to_first_token(&self) -> Option<Duration> {
        self.time_to_first_token
    }

    /// True only when the outcome holds a model response.
    pub fn success(&self) -> bool {
        self.success
    }

    /// True only for a call dropped before its outcome (a timeout, a
    /// cancellation, any drop).
    pub fn abandoned(&self) -> bool {
        self.abandoned
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn raw(value: Value) -> Option<Map<String, Value>> {
        match value {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    #[test]
    fn the_facts_are_read_from_top_level_text_members() {
        let raw = raw(json!({
            "service_tier": "standard",
            "inference_geo": "eu",
            "speed": "fast",
            "cost": 0.00042,
        }));
        assert_eq!(
            text_member(raw.as_ref(), "service_tier").as_deref(),
            Some("standard")
        );
        assert_eq!(
            text_member(raw.as_ref(), "inference_geo").as_deref(),
            Some("eu")
        );
        assert_eq!(text_member(raw.as_ref(), "speed").as_deref(), Some("fast"));
        assert_eq!(
            number_member(raw.as_ref(), "cost").and_then(|n| n.as_f64()),
            Some(0.00042)
        );
    }

    #[test]
    fn a_member_of_another_type_or_nested_is_absent() {
        let raw = raw(json!({
            "service_tier": 1,
            "inference_geo": null,
            "speed": ["fast"],
            "cost": "0.1",
            "details": { "service_tier": "nested", "cost": 3 },
        }));
        for key in ["service_tier", "inference_geo", "speed"] {
            assert_eq!(text_member(raw.as_ref(), key), None, "{key}");
        }
        assert_eq!(number_member(raw.as_ref(), "cost"), None);
        assert_eq!(text_member(None, "speed"), None);
        assert_eq!(number_member(None, "cost"), None);
    }
}
