pub mod adapter;
pub mod chart_line;
pub mod env;
pub mod error;
pub mod evidence;
pub mod expected;
pub mod floor;
pub mod guards;
pub mod ids;
pub mod keeper;
pub mod kubeconfig;
pub mod lock;
pub mod pool;
pub mod pool_guard;
pub mod pool_id;
pub mod prng;
pub mod proc;
pub mod release_chart;
pub mod restore;
pub mod saves;
pub mod seeds;
pub mod skip;
pub mod spy;
pub mod tree;
pub mod tunnel;
pub mod value_guard;
pub mod versions;
pub mod world;

use env::Env;
use error::Res;
use lock::TimingLock;
use proc::Kube;

pub struct Session {
    pub env: Env,
    pub lock: TimingLock,
}

impl Session {
    pub async fn start() -> Res<Self> {
        let env = Env::load()?;
        Kube::of(&env).reachable().await?;
        let lock = TimingLock::take(&env).await?;
        Ok(Self { env, lock })
    }
}

pub mod common;
