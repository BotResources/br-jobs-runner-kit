pub mod adapter;
pub mod env;
pub mod error;
pub mod evidence;
pub mod guards;
pub mod ids;
pub mod kubeconfig;
pub mod lock;
pub mod pool;
pub mod prng;
pub mod proc;
pub mod saves;
pub mod seeds;
pub mod spy;
pub mod tree;
pub mod tunnel;
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
