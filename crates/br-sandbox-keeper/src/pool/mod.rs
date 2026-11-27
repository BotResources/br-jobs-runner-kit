mod canonical;
mod duplicate;
mod fence;
mod names;
mod ownership;
mod quantity;
mod replicaset;
mod selector;
mod settings;
mod status;
mod template;
mod time_of_day;
mod validate;

pub use duplicate::{PoolRef, is_duplicate, pool_winner};
pub use fence::{FenceFacts, FenceFault, FenceObject, FenceProblem, FenceVerdict, fence_verdict};
pub use names::{
    ADMISSION_BINDING_NAME, POD_TEMPLATE_KEY, QUOTA_NAME, REPLICASET_PREFIX, pool_id,
    replicaset_name, template_hash_key,
};
pub use ownership::{Owner, OwnershipFault, check_ownership, owner_reference};
pub use replicaset::replica_set_body;
pub use selector::{Selector, SelectorError};
pub use settings::{FloorWindow, PoolSettings, PoolSize, ResourceBounds};
pub use status::{
    Acceptance, Counts, Observation, ReplicaSetView, STATUS_WRITE_INTERVAL, StatusInput,
    StatusWrite, compute_status, decide_write, judge_acceptance,
};
pub use template::{PoolTemplate, RenderedTemplate, TemplateFault};
pub use validate::{InvalidSpec, validate, validate_spec};
