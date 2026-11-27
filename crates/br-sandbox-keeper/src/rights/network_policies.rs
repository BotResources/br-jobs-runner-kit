pub const NETWORK_POLICY_DEFAULT_DENY: &str = "br-sandbox-default-deny";
pub const NETWORK_POLICY_EGRESS_ALLOW: &str = "br-sandbox-egress-allow";
pub const NETWORK_POLICY_EGRESS_FULL: &str = "br-sandbox-egress-full";

pub const NETWORK_POLICY_NAMES: [&str; 3] = [
    NETWORK_POLICY_DEFAULT_DENY,
    NETWORK_POLICY_EGRESS_ALLOW,
    NETWORK_POLICY_EGRESS_FULL,
];
