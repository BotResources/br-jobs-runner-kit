use std::fmt;
use std::hash::{Hash, Hasher};

macro_rules! reasons {
    ($($variant:ident => $word:literal),* $(,)?) => {
        #[derive(Debug, Clone)]
        #[non_exhaustive]
        pub enum RefusalReason {
            $($variant,)*
            Unknown(String),
        }

        impl RefusalReason {
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $word,)*
                    Self::Unknown(text) => text,
                }
            }

            pub fn from_word(word: &str) -> Self {
                match word {
                    $($word => Self::$variant,)*
                    other => Self::Unknown(other.to_owned()),
                }
            }
        }
    };
}

reasons! {
    InstanceUnknown => "instance_unknown",
    NotClaimMode => "not_claim_mode",
    InstanceDraining => "instance_draining",
    RunnerTypeRetired => "runner_type_retired",
    ClaimSpent => "claim_spent",
    MalformedClaim => "malformed_claim",
    Busy => "busy",
    RunUnknown => "run_unknown",
    RunNotStarted => "run_not_started",
    RunEnded => "run_ended",
    RunRemoved => "run_removed",
    InstanceMismatch => "instance_mismatch",
    AgentIdInOtherTree => "agent_id_in_other_tree",
    AgentIdNotFinished => "agent_id_not_finished",
    ItemNotYours => "item_not_yours",
    Malformed => "malformed",
    RunHeld => "run_held",
    RunReservedElsewhere => "run_reserved_elsewhere",
    RunNotPending => "run_not_pending",
    NotRequester => "not_requester",
    RunnerNotWorkingTheJob => "runner_not_working_the_job",
    ActingRunMissing => "acting_run_missing",
    ActingRunNotLive => "acting_run_not_live",
    JobUnknown => "job_unknown",
    JobTerminal => "job_terminal",
    PassportRequired => "passport_required",
    PassportUserMismatch => "passport_user_mismatch",
    PassportParentMismatch => "passport_parent_mismatch",
    InitialJobMismatch => "initial_job_mismatch",
    ParentTerminal => "parent_terminal",
    ParentJobUnknown => "parent_job_unknown",
    InvalidPriority => "invalid_priority",
    PayloadTooLarge => "payload_too_large",
    PayloadNotUploaded => "payload_not_uploaded",
    PayloadUploaderMismatch => "payload_uploader_mismatch",
    ModeRequired => "mode_required",
    CallerAddressNotHeld => "caller_address_not_held",
    CallerAddressNotOfRun => "caller_address_not_of_run",
    MaxAttemptsAboveCeiling => "max_attempts_above_ceiling",
    IdReuse => "id_reuse",
    DuplicateActiveEntity => "duplicate_active_entity",
    MalformedPayload => "malformed_payload",
    MailNotYours => "mail_not_yours",
}

impl PartialEq for RefusalReason {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for RefusalReason {}

impl Hash for RefusalReason {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq<str> for RefusalReason {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for RefusalReason {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for RefusalReason {
    fn from(word: &str) -> Self {
        Self::from_word(word)
    }
}

impl From<String> for RefusalReason {
    fn from(word: String) -> Self {
        Self::from_word(&word)
    }
}

#[cfg(test)]
mod tests {
    use contract_jobs::runner::ClaimRefusal;
    use contract_jobs::runner_surface::SurfaceReason;

    use super::RefusalReason;

    fn is_typed(word: &str) -> bool {
        !matches!(RefusalReason::from_word(word), RefusalReason::Unknown(_))
    }

    #[test]
    fn given_every_word_the_wire_library_names_then_it_is_a_typed_reason() {
        let surface = [
            SurfaceReason::RunUnknown,
            SurfaceReason::RunNotStarted,
            SurfaceReason::RunEnded,
            SurfaceReason::RunRemoved,
            SurfaceReason::InstanceMismatch,
            SurfaceReason::AgentIdInOtherTree,
            SurfaceReason::AgentIdNotFinished,
            SurfaceReason::ItemNotYours,
            SurfaceReason::Malformed,
            SurfaceReason::InstanceUnknown,
            SurfaceReason::RunHeld,
            SurfaceReason::RunReservedElsewhere,
            SurfaceReason::RunNotPending,
        ];
        for reason in surface {
            let word = serde_json::to_value(reason).expect("json");
            assert!(is_typed(word.as_str().expect("word")), "{word}");
        }
        for reason in ClaimRefusal::ALL {
            assert!(is_typed(reason.as_str()), "{}", reason.as_str());
        }
        let rejections = [
            contract_jobs::event::REASON_ID_REUSE,
            contract_jobs::event::REASON_DUPLICATE_ACTIVE_ENTITY,
            contract_jobs::event::REASON_MALFORMED_PAYLOAD,
            contract_jobs::event_v2::REASON_NOT_REQUESTER,
            contract_jobs::event_v2::REASON_RUNNER_NOT_WORKING_THE_JOB,
            contract_jobs::event_v2::REASON_ACTING_RUN_MISSING,
            contract_jobs::event_v2::REASON_ACTING_RUN_NOT_LIVE,
            contract_jobs::event_v2::REASON_JOB_UNKNOWN,
            contract_jobs::event_v2::REASON_JOB_TERMINAL,
            contract_jobs::event_v2::REASON_PASSPORT_REQUIRED,
            contract_jobs::event_v2::REASON_PASSPORT_USER_MISMATCH,
            contract_jobs::event_v2::REASON_PASSPORT_PARENT_MISMATCH,
            contract_jobs::event_v2::REASON_INITIAL_JOB_MISMATCH,
            contract_jobs::event_v2::REASON_PARENT_TERMINAL,
            contract_jobs::event_v2::REASON_PARENT_JOB_UNKNOWN,
            contract_jobs::event_v2::REASON_RUNNER_TYPE_RETIRED,
            contract_jobs::event_v2::REASON_INVALID_PRIORITY,
            contract_jobs::event_v2::REASON_PAYLOAD_TOO_LARGE,
            contract_jobs::event_v2::REASON_PAYLOAD_NOT_UPLOADED,
            contract_jobs::event_v2::REASON_PAYLOAD_UPLOADER_MISMATCH,
            contract_jobs::event_v2::REASON_MODE_REQUIRED,
            contract_jobs::event_v2::REASON_CALLER_ADDRESS_NOT_HELD,
            contract_jobs::event_v2::REASON_CALLER_ADDRESS_NOT_OF_RUN,
            contract_jobs::event_v2::REASON_MAX_ATTEMPTS_ABOVE_CEILING,
        ];
        for word in rejections {
            assert!(is_typed(word), "{word}");
        }
    }

    #[test]
    fn given_a_known_word_when_made_then_it_is_the_typed_reason_and_keeps_its_word() {
        let reason = RefusalReason::from_word("run_held");
        assert!(matches!(reason, RefusalReason::RunHeld));
        assert_eq!(reason.as_str(), "run_held");
        assert_eq!(reason.to_string(), "run_held");
    }

    #[test]
    fn given_an_unknown_word_when_made_then_the_text_is_kept_and_no_reason_is_guessed() {
        let reason = RefusalReason::from_word("brand_new_reason");
        assert!(matches!(&reason, RefusalReason::Unknown(text) if text == "brand_new_reason"));
        assert_ne!(reason, RefusalReason::Busy);
    }

    #[test]
    fn given_the_unknown_case_holding_a_known_word_then_it_equals_the_typed_reason() {
        assert_eq!(
            RefusalReason::Unknown("busy".to_owned()),
            RefusalReason::Busy
        );
    }

    #[test]
    fn given_a_word_shared_by_two_operations_then_one_reason_serves_both() {
        assert_eq!(
            RefusalReason::from_word("runner_type_retired"),
            RefusalReason::RunnerTypeRetired
        );
        assert_eq!(
            RefusalReason::from_word("instance_unknown"),
            RefusalReason::InstanceUnknown
        );
    }
}
