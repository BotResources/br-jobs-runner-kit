use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::env::Env;
use crate::error::{BatteryError, Res};

const WAIT_SECONDS: u64 = 6 * 3600;
const PID_FILE: &str = "pid";

pub struct TimingLock {
    path: PathBuf,
}

fn file_error(path: &Path, error: impl ToString) -> BatteryError {
    BatteryError::File {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

fn holder_of(lock: &Path) -> Option<u32> {
    std::fs::read_to_string(lock.join(PID_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(true)
}

fn remove_lock(lock: &Path) {
    let _ = std::fs::remove_file(lock.join(PID_FILE));
    let _ = std::fs::remove_dir(lock);
}

fn take_over_if_dead(lock: &Path) -> bool {
    let Some(pid) = holder_of(lock) else {
        return false;
    };
    if is_alive(pid) {
        return false;
    }
    let moved = lock.with_extension(format!("stale-{}", std::process::id()));
    if std::fs::rename(lock, &moved).is_err() {
        return false;
    }
    if holder_of(&moved) != Some(pid) {
        let _ = std::fs::rename(&moved, lock);
        return false;
    }
    eprintln!(
        "timing lock {}: the holder with pid {pid} is gone, the lock is taken over",
        lock.display()
    );
    remove_lock(&moved);
    true
}

impl TimingLock {
    pub async fn take(env: &Env) -> Res<Self> {
        let path = &env.timing_lock;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut waited = 0;
        loop {
            match std::fs::create_dir(path) {
                Ok(()) => {
                    if let Err(error) =
                        std::fs::write(path.join(PID_FILE), std::process::id().to_string())
                    {
                        remove_lock(path);
                        return Err(file_error(path, error));
                    }
                    return Ok(Self { path: path.clone() });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if take_over_if_dead(path) {
                        continue;
                    }
                    if waited >= WAIT_SECONDS {
                        return Err(BatteryError::LockBusy {
                            path: path.display().to_string(),
                            seconds: WAIT_SECONDS,
                        });
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    waited += 2;
                }
                Err(error) => return Err(file_error(path, error)),
            }
        }
    }
}

impl Drop for TimingLock {
    fn drop(&mut self) {
        remove_lock(&self.path);
    }
}
