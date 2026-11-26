use std::collections::HashSet;

use br_agentic_harness_contract::{PortError, Site, WebAccess};

use crate::config::NetworkRule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the web access of the request does not match the network mode of the adapter")]
pub struct WebAccessMismatch;

impl From<WebAccessMismatch> for PortError {
    fn from(error: WebAccessMismatch) -> Self {
        PortError::refused(error.to_string())
    }
}

pub fn check_web_access(
    rule: &NetworkRule,
    request: Option<&WebAccess>,
) -> Result<(), WebAccessMismatch> {
    let accepted = match (rule, request) {
        (NetworkRule::None, None) => true,
        (NetworkRule::None, Some(access)) => access.reaches_nothing(),
        (NetworkRule::Full, Some(WebAccess::Full)) => true,
        (NetworkRule::Allow(allowed), Some(WebAccess::Whitelist(sites))) => {
            sites.iter().collect::<HashSet<&Site>>() == allowed.iter().collect()
        }
        _ => false,
    };
    if accepted {
        Ok(())
    } else {
        Err(WebAccessMismatch)
    }
}
