use uuid::Uuid;

use crate::error::Error;

pub(crate) fn require_v7(id: Uuid, what: &str) -> Result<Uuid, Error> {
    if id.get_version_num() == 7 {
        Ok(id)
    } else {
        Err(Error::invalid(format!("{what} is not a UUIDv7")))
    }
}
