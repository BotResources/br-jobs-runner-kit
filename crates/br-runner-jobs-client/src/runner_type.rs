use std::fmt;
use std::str::FromStr;

use contract_jobs::segment::{SegmentError, SubjectSegment};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunnerType(SubjectSegment);

pub(crate) fn segment_error(error: &SegmentError) -> Error {
    let field = match error {
        SegmentError::Blank { field }
        | SegmentError::TooLong { field, .. }
        | SegmentError::Invalid { field, .. } => field,
    };
    Error::invalid(format!(
        "the {field} is not a valid segment: {}",
        error.code()
    ))
}

impl RunnerType {
    pub fn new(value: &str) -> Result<Self, Error> {
        SubjectSegment::runner_type(value)
            .map(Self)
            .map_err(|error| segment_error(&error))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn segment(&self) -> &SubjectSegment {
        &self.0
    }
}

impl fmt::Display for RunnerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for RunnerType {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for RunnerType {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl AsRef<str> for RunnerType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for RunnerType {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for RunnerType {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::RunnerType;
    use crate::error::Error;

    #[test]
    fn given_a_value_the_wire_accepts_when_made_then_it_keeps_its_text() {
        let runner_type = RunnerType::new("analyst").expect("a runner type");
        assert_eq!(runner_type.as_str(), "analyst");
        assert_eq!(runner_type.to_string(), "analyst");
        assert_eq!("analyst".parse::<RunnerType>(), Ok(runner_type));
    }

    #[test]
    fn given_a_value_the_wire_refuses_when_made_then_invalid_input() {
        for value in ["", "a.b", "a b", "a*", "a>", "  "] {
            assert!(
                matches!(RunnerType::new(value), Err(Error::InvalidInput(_))),
                "{value:?}"
            );
        }
    }
}
