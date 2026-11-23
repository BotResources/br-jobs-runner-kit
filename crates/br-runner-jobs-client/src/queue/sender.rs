use crate::backend::BoxFuture;
use crate::error::Error;
use crate::queue::write::Write;

pub(crate) trait WriteSession: Send {
    fn send(&mut self) -> BoxFuture<'_, Result<(), Error>>;
}

pub(crate) trait WriteSender: Send + Sync {
    fn send<'a>(&'a self, write: &'a Write) -> BoxFuture<'a, Result<(), Error>>;

    fn session<'a>(&'a self, write: &'a Write) -> Box<dyn WriteSession + 'a>;
}

pub(crate) struct Direct<'a, S: ?Sized> {
    pub sender: &'a S,
    pub write: &'a Write,
}

impl<S: WriteSender + ?Sized> WriteSession for Direct<'_, S> {
    fn send(&mut self) -> BoxFuture<'_, Result<(), Error>> {
        self.sender.send(self.write)
    }
}
