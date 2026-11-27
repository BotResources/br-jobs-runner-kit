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
    let Ok(output) = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
    else {
        return true;
    };
    if output.status.success() {
        return true;
    }
    !String::from_utf8_lossy(&output.stderr)
        .to_ascii_lowercase()
        .contains("no such process")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("battery-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn a_lock_whose_holder_is_gone_is_taken_over() {
        let lock = lock_folder("dead");
        let pid = dead_pid();
        std::fs::write(lock.join(PID_FILE), pid.to_string()).unwrap();
        assert!(take_over_if_dead(&lock));
        assert!(!lock.exists());
    }

    #[test]
    fn a_lock_whose_holder_is_alive_is_not_taken_over() {
        let lock = lock_folder("alive");
        std::fs::write(lock.join(PID_FILE), std::process::id().to_string()).unwrap();
        assert!(!take_over_if_dead(&lock));
        assert_eq!(holder_of(&lock), Some(std::process::id()));
        remove_lock(&lock);
    }

    #[test]
    fn a_lock_without_a_pid_file_is_never_taken_over() {
        let lock = lock_folder("nopid");
        assert!(!take_over_if_dead(&lock));
        assert!(lock.exists());
        remove_lock(&lock);
    }

    #[test]
    fn a_live_process_of_another_user_does_not_count_as_gone() {
        assert!(is_alive(1));
    }

    #[test]
    fn a_process_that_does_not_exist_counts_as_gone() {
        assert!(!is_alive(dead_pid()));
    }
}
