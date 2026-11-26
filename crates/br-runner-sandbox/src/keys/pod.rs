use crate::config::{DEFAULT_LABEL_PREFIX, LabelPrefix};

pub const LABEL_ROLE: &str = "br.sandbox/role";
pub const LABEL_POOL_ID: &str = "br.sandbox/pool-id";
pub const LABEL_POOL: &str = "br.sandbox/pool";
pub const LABEL_THREAD: &str = "br.sandbox/thread";
pub const LABEL_AGENT: &str = "br.sandbox/agent";
pub const LABEL_MANAGED: &str = "br.sandbox/managed";

pub const ANNOTATION_CLEAN: &str = "br.sandbox/clean";
pub const ANNOTATION_CLAIMED_AT: &str = "br.sandbox/claimed-at";
pub const ANNOTATION_SEED_HASH: &str = "br.sandbox/seed-hash";
pub const ANNOTATION_STATE_TURN: &str = "br.sandbox/state-turn";
pub const ANNOTATION_SAVED_AT: &str = "br.sandbox/saved-at";
pub const ANNOTATION_STATE_BYTES: &str = "br.sandbox/state-bytes";
pub const ANNOTATION_IDLE_SINCE: &str = "br.sandbox/idle-since";

pub const ROLE_SANDBOX: &str = "sandbox";
pub const POOL_WARM: &str = "warm";
pub const POOL_CLAIMED: &str = "claimed";
pub const CLEAN_TRUE: &str = "true";
pub const CLEAN_FALSE: &str = "false";
pub const MANAGED_TRUE: &str = "true";

pub const CONDITION_WARM_PODS_FAILING: &str = "WarmPodsFailing";
pub const REASON_IMAGE_PULL_FAILING: &str = "ImagePullFailing";
pub const REASON_START_CRASHING: &str = "StartCrashing";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodKeys {
    prefix: LabelPrefix,
}

impl PodKeys {
    pub fn new(prefix: LabelPrefix) -> Self {
        Self { prefix }
    }

    pub fn prefix(&self) -> &LabelPrefix {
        &self.prefix
    }

    fn key(&self, default: &str) -> String {
        let name = default
            .strip_prefix(DEFAULT_LABEL_PREFIX)
            .unwrap_or(default);
        format!("{}{name}", self.prefix)
    }

    pub fn role(&self) -> String {
        self.key(LABEL_ROLE)
    }

    pub fn pool_id(&self) -> String {
        self.key(LABEL_POOL_ID)
    }

    pub fn pool(&self) -> String {
        self.key(LABEL_POOL)
    }

    pub fn thread(&self) -> String {
        self.key(LABEL_THREAD)
    }

    pub fn agent(&self) -> String {
        self.key(LABEL_AGENT)
    }

    pub fn managed(&self) -> String {
        self.key(LABEL_MANAGED)
    }

    pub fn clean(&self) -> String {
        self.key(ANNOTATION_CLEAN)
    }

    pub fn claimed_at(&self) -> String {
        self.key(ANNOTATION_CLAIMED_AT)
    }

    pub fn seed_hash(&self) -> String {
        self.key(ANNOTATION_SEED_HASH)
    }

    pub fn state_turn(&self) -> String {
        self.key(ANNOTATION_STATE_TURN)
    }

    pub fn saved_at(&self) -> String {
        self.key(ANNOTATION_SAVED_AT)
    }

    pub fn state_bytes(&self) -> String {
        self.key(ANNOTATION_STATE_BYTES)
    }

    pub fn idle_since(&self) -> String {
        self.key(ANNOTATION_IDLE_SINCE)
    }
}

impl Default for PodKeys {
    fn default() -> Self {
        Self::new(LabelPrefix::default())
    }
}
