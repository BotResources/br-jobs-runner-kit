use uuid::{Uuid, Variant};

use crate::error::Error;

pub(crate) fn require_v7(id: Uuid, what: &str) -> Result<Uuid, Error> {
    if id.get_version_num() == 7 && id.get_variant() == Variant::RFC4122 {
        Ok(id)
    } else {
        Err(Error::invalid(format!("{what} is not a UUIDv7")))
    }
}

pub(crate) fn require_wait(wait: std::time::Duration, what: &str) -> Result<(), Error> {
    if wait.is_zero() {
        Err(Error::invalid(format!("{what} is zero")))
    } else {
        Ok(())
    }
}
