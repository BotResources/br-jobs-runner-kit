use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct WorkIds {
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub instance_key: String,
}

use serde_json::Value;

use crate::backend::Backend;
use crate::error::Error;
use crate::model::{Content, Mail, NewMailInput, ResultRef, SavedState, UnreadItem, ContentAttachment};

fn stub<T>() -> Result<T, Error> {
    Err(Error::invalid("not implemented yet"))
}

pub(crate) async fn payload(_b: &dyn Backend, _w: &WorkIds) -> Result<Content, Error> {
    stub()
}

pub(crate) async fn upload_payload(
    _b: &dyn Backend,
    _w: &WorkIds,
    _for_job_id: Uuid,
    _content: &Content,
) -> Result<(), Error> {
    stub()
}

pub(crate) async fn upload_result(_b: &dyn Backend, _w: &WorkIds, _answer: &Content) -> Result<(), Error> {
    stub()
}

pub(crate) async fn read_result(
    _b: &dyn Backend,
    _w: &WorkIds,
    _result: ResultRef,
    _keep: bool,
) -> Result<Content, Error> {
    stub()
}

pub(crate) async fn save_state(_b: &dyn Backend, _w: &WorkIds, _state: &Value) -> Result<(), Error> {
    stub()
}

pub(crate) async fn saved_state(_b: &dyn Backend, _w: &WorkIds) -> Result<SavedState, Error> {
    stub()
}

pub(crate) async fn send_mail(_b: &dyn Backend, _w: &WorkIds, _mail: &NewMailInput) -> Result<(), Error> {
    stub()
}

pub(crate) async fn unread(
    _b: &dyn Backend,
    _w: &WorkIds,
    _address: Option<Uuid>,
) -> Result<Vec<UnreadItem>, Error> {
    stub()
}

pub(crate) async fn read_mail(
    _b: &dyn Backend,
    _w: &WorkIds,
    _mail_id: Uuid,
    _address: Uuid,
) -> Result<Mail, Error> {
    stub()
}

pub(crate) async fn attachment(
    _b: &dyn Backend,
    _w: &WorkIds,
    _mail_id: Uuid,
    _attachment_id: Uuid,
) -> Result<ContentAttachment, Error> {
    stub()
}
