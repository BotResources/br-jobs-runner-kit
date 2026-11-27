mod network_policies;
mod table;
#[cfg(test)]
mod tests;

pub use network_policies::{
    NETWORK_POLICY_DEFAULT_DENY, NETWORK_POLICY_EGRESS_ALLOW, NETWORK_POLICY_EGRESS_FULL,
    NETWORK_POLICY_NAMES,
};
pub use table::{POD_TEMPLATE_CONFIGMAP, RIGHTS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RightScope {
    EachManagedNamespace,
    LeaseNamespace,
    Cluster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Right {
    pub scope: RightScope,
    pub api_group: &'static str,
    pub resource: &'static str,
    pub resource_names: &'static [&'static str],
    pub verbs: &'static [&'static str],
}
