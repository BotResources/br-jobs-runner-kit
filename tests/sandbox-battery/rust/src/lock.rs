use std::path::PathBuf;
use std::time::Duration;

use crate::env::Env;
use crate::error::{BatteryError, Res};

const WAIT_SECONDS: u64 = 6 * 3600;

pub struct TimingLock {
    path: PathBuf,
}

impl TimingLock {
    pub async fn take(env: &Env) -> Res<Self> {
        if let Some(parent) = env.timing_lock.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut waited = 0;
        loop {
            match std::fs::create_dir(&env.timing_lock) {
                Ok(()) => {
                    return Ok(Self {
                        path: env.timing_lock.clone(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if waited >= WAIT_SECONDS {
                        return Err(BatteryError::LockBusy {
                            path: env.timing_lock.display().to_string(),
                            seconds: WAIT_SECONDS,
                        });
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    waited += 2;
                }
                Err(error) => {
                    return Err(BatteryError::File {
                        path: env.timing_lock.display().to_string(),
                        reason: error.to_string(),
                    });
                }
            }
        }
    }
}

impl Drop for TimingLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}
