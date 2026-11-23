use crate::backend::RawFeed;
use crate::error::Error;

pub struct Feed<T> {
    source: Box<dyn RawFeed<T>>,
    ended: Option<Error>,
}

impl<T> Feed<T> {
    pub(crate) fn new(source: Box<dyn RawFeed<T>>) -> Self {
        Self {
            source,
            ended: None,
        }
    }

    pub async fn next(&mut self) -> Result<T, Error> {
        if let Some(error) = &self.ended {
            return Err(error.clone());
        }
        match self.source.next().await {
            Ok(event) => Ok(event),
            Err(error) => {
                if !matches!(error, Error::Unreadable(_)) {
                    self.ended = Some(error.clone());
                }
                Err(error)
            }
        }
    }
}

impl<T> std::fmt::Debug for Feed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feed")
            .field("ended", &self.ended.is_some())
            .finish()
    }
}
