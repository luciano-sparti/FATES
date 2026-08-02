use crate::error::Error;
use nix::fcntl::{Flock, FlockArg};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = "state.lock";
const TMP_FILE: &str = ".state.json.tmp";

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct State {
    pub processes: HashMap<String, ProcessState>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProcessState {
    pub pid: Option<u32>,
    pub cmd: String,
    /// Fully split command ready to spawn (executable + arguments).
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

/// An exclusive advisory lock guarding read-modify-write access to the state
/// file. Held until dropped (the kernel releases the lock when the fd closes,
/// even on process exit).
pub struct StateLock {
    _lock: Flock<std::fs::File>,
}

impl StateLock {
    /// Block until the lock is acquired. Creates the state directory and lock
    /// file if they do not exist yet.
    pub fn acquire(dir: &Path) -> Result<Self, Error> {
        fs::create_dir_all(dir).map_err(Error::from)?;
        let lock_path = dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| {
                Error::system(format!(
                    "Failed to open state lock '{}': {}",
                    lock_path.display(),
                    e
                ))
            })?;
        let lock = Flock::lock(file, FlockArg::LockExclusive).map_err(|(_, e)| {
            Error::system(format!(
                "Failed to lock state in '{}': {}",
                dir.display(),
                e
            ))
        })?;
        Ok(StateLock { _lock: lock })
    }
}

impl State {
    pub fn state_file(dir: &Path) -> PathBuf {
        dir.join("state.json")
    }

    pub fn log_file(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{}.log", name))
    }

    pub fn load(dir: &Path) -> Self {
        let path = Self::state_file(dir);
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(state) => return state,
                    Err(e) => eprintln!(
                        "Warning: state file '{}' is corrupt ({}); starting fresh.",
                        path.display(),
                        e
                    ),
                },
                Err(e) => eprintln!(
                    "Warning: could not read state file '{}': {}; starting fresh.",
                    path.display(),
                    e
                ),
            }
        }
        State::default()
    }

    /// Persist atomically: write to a temp file, then rename over the real
    /// state file. A crash mid-write can never leave a truncated state file.
    pub fn save(&self, dir: &Path) -> Result<(), Error> {
        fs::create_dir_all(dir).map_err(Error::from)?;
        let content = serde_json::to_string_pretty(self).map_err(Error::from)?;
        let tmp = dir.join(TMP_FILE);
        fs::write(&tmp, content).map_err(Error::from)?;
        fs::rename(&tmp, Self::state_file(dir)).map_err(Error::from)?;
        Ok(())
    }
}

/// Returns `true` when we can positively determine that `pid` no longer refers
/// to a process launched with `args` — the process is gone, or its PID has
/// been reused by an unrelated process (verified against `/proc/<pid>/cmdline`).
///
/// On platforms without `/proc` (identity cannot be verified) this returns
/// `false`, so callers fall back to trusting the PID as before.
pub fn pid_is_stale(pid: u32, args: &[String]) -> bool {
    if args.is_empty() {
        return true;
    }
    let proc_root = Path::new("/proc");
    if !proc_root.exists() {
        return false;
    }
    let Ok(data) = fs::read(proc_root.join(pid.to_string()).join("cmdline")) else {
        return true;
    };
    let actual: Vec<&[u8]> = data.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
    actual.len() != args.len()
        || actual
            .iter()
            .zip(args.iter())
            .any(|(a, b)| *a != b.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    fn spawn_sleep() -> (std::process::Child, u32) {
        let child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        thread::sleep(Duration::from_millis(50));
        (child, pid)
    }

    fn kill_and_reap(child: std::process::Child) {
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn live_matching_process_is_not_stale() {
        let (child, pid) = spawn_sleep();
        let args = vec!["sleep".to_string(), "60".to_string()];
        assert!(!pid_is_stale(pid, &args));
        kill_and_reap(child);
    }

    #[test]
    fn dead_process_is_stale() {
        let (child, pid) = spawn_sleep();
        kill_and_reap(child);
        thread::sleep(Duration::from_millis(50));
        assert!(pid_is_stale(pid, &["sleep".to_string(), "60".to_string()]));
    }

    #[test]
    fn differing_cmdline_is_stale() {
        let (child, pid) = spawn_sleep();
        assert!(pid_is_stale(
            pid,
            &["something-else".to_string(), "60".to_string()]
        ));
        kill_and_reap(child);
    }

    #[test]
    fn empty_args_are_stale() {
        assert!(pid_is_stale(1234, &[]));
    }
}
