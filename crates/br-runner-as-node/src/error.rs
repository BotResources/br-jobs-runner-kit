use br_graph_contract::Cause;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("the caller source gave an error")]
    CallerSource(#[source] Cause),
    #[error("the caller handle could not take the upload of the payload")]
    Upload(#[source] Cause),
    #[error("the caller handle could not take the create of the child Job")]
    Create(#[source] Cause),
    #[error("the create of the child Job was removed after a report: {what}")]
    CreateRemoved {
        what: String,
        #[source]
        source: Option<Cause>,
    },
    #[error("the end wait gave an error")]
    EndWait(#[source] Cause),
    #[error("the end of the child Job is of a kind this library does not know")]
    UnknownEnd,
}
