use crate::backend::BoxFuture;
use crate::error::Error;
use crate::queue::write::Write;

pub(crate) trait WriteSender: Send + Sync {
    fn send<'a>(&'a self, write: &'a Write) -> BoxFuture<'a, Result<(), Error>>;
}
