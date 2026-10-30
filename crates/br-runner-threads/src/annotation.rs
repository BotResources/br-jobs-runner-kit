use contract_threads::{
    Annotation as WireAnnotation, AnnotationBody as WireBody, ArtifactRef, JobRef, Notice,
    ThreadRef as WireThreadRef,
};
use uuid::Uuid;

use crate::error::Error;
use crate::ids::require_v7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationBody {
    Artifact { artifact_id: Uuid },
    ThreadReference { thread_id: Uuid },
    JobReference { job_id: Uuid },
    Error { code: String, text: Option<String> },
    Warning { code: String, text: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub id: Uuid,
    pub body: AnnotationBody,
}

impl Annotation {
    pub fn new(id: Uuid, body: AnnotationBody) -> Self {
        Self { id, body }
    }

    pub(crate) fn to_wire(&self) -> Result<WireAnnotation, Error> {
        require_v7(self.id, "an annotation id")?;
        let body = match &self.body {
            AnnotationBody::Artifact { artifact_id } => WireBody::Artifact(ArtifactRef {
                artifact_id: *artifact_id,
            }),
            AnnotationBody::ThreadReference { thread_id } => {
                WireBody::ThreadReference(WireThreadRef {
                    thread_id: *thread_id,
                })
            }
            AnnotationBody::JobReference { job_id } => {
                WireBody::JobReference(JobRef { job_id: *job_id })
            }
            AnnotationBody::Error { code, text } => WireBody::Error(notice(code, text)),
            AnnotationBody::Warning { code, text } => WireBody::Warning(notice(code, text)),
        };
        Ok(WireAnnotation::new(self.id, body))
    }
}

fn notice(code: &str, text: &Option<String>) -> Notice {
    Notice {
        code: code.to_owned(),
        text: text.clone(),
    }
}
