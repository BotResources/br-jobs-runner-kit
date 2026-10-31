use br_agentic_contract::CancelView;
use br_llm_messages::OpaqueJson;
use br_runner_jobs::{Content, PayloadForm};

use super::race::unless_cancelled;
use super::{RunnerToolCode, Stop};
use crate::author::{AuthorFailure, Payload};
use crate::mode::PayloadKind;

pub(super) struct Prepared {
    pub(super) form: PayloadForm,
    pub(super) upload: Option<Content>,
}

impl RunnerToolCode {
    pub(super) async fn prepare(
        &self,
        arguments: &OpaqueJson,
        cancel: &CancelView,
    ) -> Result<Prepared, Stop> {
        let Some(builder) = &self.builder else {
            return self.with_content(arguments_content(arguments)?);
        };
        let built = unless_cancelled(cancel, builder(arguments.clone()))
            .await
            .ok_or(Stop::Cancelled)?;
        match built {
            Ok(Payload::Content(content)) => self.with_content(content),
            Ok(Payload::Reference {
                service,
                document_id,
            }) => Ok(Prepared {
                form: PayloadForm::Reference {
                    service,
                    document_id,
                },
                upload: None,
            }),
            Err(AuthorFailure::Refused(text)) => Err(Stop::Flag(text)),
            Err(AuthorFailure::Fault(cause)) => Err(Stop::Fault(cause)),
        }
    }

    fn with_content(&self, content: Content) -> Result<Prepared, Stop> {
        Ok(match self.kind {
            PayloadKind::Kept => Prepared {
                form: PayloadForm::Kept,
                upload: Some(content),
            },
            PayloadKind::Inline => Prepared {
                form: PayloadForm::Inline(content),
                upload: None,
            },
        })
    }
}

fn arguments_content(arguments: &OpaqueJson) -> Result<Content, Stop> {
    let message = arguments.to_json_string().map_err(Stop::fault)?;
    Ok(Content {
        message,
        attachments: Vec::new(),
    })
}
