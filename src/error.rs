use std::fmt;

/// Errors produced by the fates CLI, mapped to process exit codes by `main`.
#[derive(Debug)]
pub enum Error {
    /// Expected, user-facing errors: unknown groups, invalid config, a process
    /// already running or not running, missing log files. Exit code 1.
    Command(String),
    /// System-level failures: I/O, serialization, spawning, signalling, state
    /// locking. Exit code 2.
    System(String),
}

impl Error {
    pub fn command<S: Into<String>>(msg: S) -> Self {
        Error::Command(msg.into())
    }

    pub fn system<S: Into<String>>(msg: S) -> Self {
        Error::System(msg.into())
    }

    /// The process exit code associated with this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Command(_) => 1,
            Error::System(_) => 2,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Command(msg) => f.write_str(msg),
            Error::System(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::system(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::system(e.to_string())
    }
}
