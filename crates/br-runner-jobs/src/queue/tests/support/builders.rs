use super::*;

pub const RUN: u128 = 1;
pub const START: u128 = 2;
pub const WORK_JOB: u128 = 3;
pub const PARENT: u128 = 4;
pub const INITIAL: u128 = 5;
pub const PASSPORT_TEXT: &str = "PASSPORT-SENTINEL-8841";
pub const BODY_TEXT: &str = "BODY-SENTINEL-7730";

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub fn content(text: &str) -> Content {
    Content {
        message: text.to_owned(),
        attachments: Vec::new(),
    }
}

pub fn registration(agent: u128) -> Write {
    Write::Registration {
        run_id: id(RUN),
        agent: AgentDeclaration {
            agent_id: id(agent),
            has_inbox: true,
            can_restart: false,
        },
    }
}

pub fn mail(mail_id: u128, from: u128, to: u128) -> Write {
    Write::Mail {
        mail: NewMailInput {
            mail_id: id(mail_id),
            from_address: id(from),
            author: "author".to_owned(),
            source_conversation: id(900),
            subject: "subject".to_owned(),
            body: BODY_TEXT.to_owned(),
            to: vec![id(to)],
            cc: Vec::new(),
            attachments: Vec::new(),
        },
    }
}

pub fn marker(item: u128, agent: u128) -> Write {
    Write::Marker {
        run_id: id(RUN),
        item_id: id(item),
        agent_id: id(agent),
        entry_id: id(item + 500),
    }
}

pub fn upload(job: u128) -> Write {
    Write::Upload {
        job_id: id(job),
        content: content(BODY_TEXT),
    }
}

pub fn create(job: u128, command: u128) -> Write {
    Write::Create(CreateChild {
        command_id: id(command),
        job_id: id(job),
        runner_type: "child".to_owned(),
        mode: CallMode::Sync,
        config: None,
        payload: PayloadForm::Kept,
        passport: json!({ "sentinel": PASSPORT_TEXT }),
        parent_job_id: id(PARENT),
        initial_job_id: id(INITIAL),
        run_id: id(RUN),
        max_attempts: None,
    })
}

pub fn background_create(job: u128, command: u128, caller: u128) -> Write {
    match create(job, command) {
        Write::Create(mut child) => {
            child.mode = CallMode::Background {
                caller_address: id(caller),
            };
            Write::Create(child)
        }
        other => other,
    }
}

pub fn finish(job: u128, command: u128) -> Write {
    Write::Finish {
        run_id: id(RUN),
        job_id: id(job),
        command_id: id(command),
    }
}

pub fn cancel(job: u128, command: u128) -> Write {
    Write::Cancel {
        run_id: id(RUN),
        job_id: id(job),
        command_id: id(command),
    }
}

pub fn answer() -> Write {
    Write::Answer(content(BODY_TEXT))
}

pub fn saved() -> Write {
    Write::SavedState(json!({ "kept": BODY_TEXT }))
}

pub fn plan() -> Write {
    Write::Fact(Fact::Plan {
        run_id: id(RUN),
        steps: vec!["one".to_owned()],
        declaration_id: Some(id(77)),
    })
}

pub fn step(index: u32) -> Write {
    Write::Fact(Fact::Step {
        run_id: id(RUN),
        index,
        label: "label".to_owned(),
        started_at: chrono::DateTime::UNIX_EPOCH,
    })
}

pub fn completed() -> Write {
    Write::Fact(Fact::Completed { run_id: id(RUN) })
}

pub fn failed_fact() -> Write {
    Write::Fact(Fact::Failed {
        run_id: id(RUN),
        failed: Failed {
            kind: FailureKind::Permanent,
            reason_code: "code".to_owned(),
            params: json!({}),
            diagnostic: json!({}),
            retry_after_seconds: None,
        },
    })
}

pub fn unavailable() -> Error {
    Error::unavailable("no answer")
}

pub fn lost() -> Error {
    Error::lost("cut")
}

pub fn unreadable() -> Error {
    Error::Unreadable("garbled".to_owned())
}

pub fn fetch_failed() -> Error {
    Error::Credential(CredentialFailure {
        kind: CredentialKind::FetchFailed,
        text: "provider down".to_owned(),
    })
}

pub fn no_usable() -> Error {
    Error::Credential(CredentialFailure {
        kind: CredentialKind::NoUsable,
        text: "empty token".to_owned(),
    })
}

pub fn refusal(operation: Operation, reason: &str) -> Error {
    Error::refused(operation, reason, "jobs text for agents")
}

pub fn fail(error: Error) -> Step {
    Step::Reply(Err(error))
}
