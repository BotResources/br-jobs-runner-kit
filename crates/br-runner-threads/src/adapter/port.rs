use std::sync::Arc;

use br_agentic_contract::{Fields, PortError, PortFuture, ThreadPort, ThreadRef};
use uuid::Uuid;

use crate::error::Error;

type Finder<F> =
    dyn for<'a> Fn(&'a F) -> PortFuture<'a, Result<Option<Uuid>, PortError>> + Send + Sync;

pub struct ThreadPortAdapter<F: Fields> {
    find: Arc<Finder<F>>,
}

impl<F: Fields> ThreadPortAdapter<F> {
    pub fn new<C>(find: C) -> Self
    where
        C: for<'a> Fn(&'a F) -> PortFuture<'a, Result<Option<Uuid>, PortError>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            find: Arc::new(find),
        }
    }
}

impl<F: Fields> ThreadPort<F> for ThreadPortAdapter<F> {
    fn thread<'a>(&'a self, snapshot: &'a F) -> PortFuture<'a, Result<ThreadRef, PortError>> {
        Box::pin(async move {
            let id = (self.find)(snapshot).await?.ok_or_else(|| {
                PortError::fault(Error::invalid("the state snapshot holds no thread id"))
            })?;
            ThreadRef::from_uuid(id).map_err(|e| PortError::fault(Error::invalid(e.to_string())))
        })
    }
}
