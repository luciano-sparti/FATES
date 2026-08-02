use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "fates", about = "Thread management, orchestrated. ⎊", version)]
pub struct Cli {
    /// Path to fates.yaml config file
    #[arg(long, short, global = true, default_value = "fates.yaml")]
    pub config: String,

    /// Directory for the state file and per-service logs.
    /// Defaults to $FATES_STATE_DIR, then /tmp/fates.
    #[arg(long, global = true)]
    pub state_dir: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create a named process group
    #[command(visible_alias = "register")]
    Spin {
        /// The name of the process group
        name: String,

        /// Command and arguments to run (optional if using fates.yaml)
        #[arg(last = true)]
        cmd: Vec<String>,
    },
    /// Start / resume a process group in the background
    #[command(visible_alias = "start")]
    Draw {
        /// The name of the process group (omit with --all)
        #[arg(required_unless_present = "all")]
        name: Option<String>,

        /// Start every registered process group, in dependency order
        #[arg(long)]
        all: bool,
    },
    /// Gracefully or forcefully stop a process group
    #[command(visible_alias = "stop")]
    Cut {
        /// The name of the process group (omit with --all)
        #[arg(required_unless_present = "all")]
        name: Option<String>,

        /// Stop every registered process group, dependents first
        #[arg(long)]
        all: bool,

        /// Force stop immediately with SIGKILL, skipping graceful SIGTERM
        #[arg(long)]
        force: bool,
    },
    /// One-line dashboard: all groups, status, CPU%, lifetime
    #[command(visible_alias = "status")]
    Loom {
        /// Emit machine-readable JSON instead of the table
        #[arg(long)]
        json: bool,

        /// Refresh every N seconds (default: 2). `--watch` alone uses 2s.
        #[arg(long, short, num_args = 0..=1, default_missing_value = "2")]
        watch: Option<u64>,
    },
    /// ASCII dependency tree of a process group
    #[command(visible_alias = "tree")]
    Weave {
        /// The name of the process group
        name: String,
    },
    /// Inspect a running group's lifecycle, logs, and resources
    #[command(visible_alias = "info")]
    Omen {
        /// The name of the process group
        name: String,
    },
    /// Print captured logs for a process group
    #[command(visible_alias = "log")]
    Logs {
        /// The name of the process group
        name: String,

        /// Number of lines to show from the end (default: all)
        #[arg(long, short)]
        tail: Option<usize>,

        /// Keep following the log as it grows (like `tail -f`)
        #[arg(long, short)]
        follow: bool,
    },
    /// Wait for a process group to stop, then exit (useful in scripts)
    Wait {
        /// The name of the process group
        name: String,

        /// Give up after N seconds
        #[arg(long, short)]
        timeout: Option<u64>,
    },
    /// Write a starter fates.yaml template
    Init {
        /// Path to write (default: fates.yaml)
        #[arg(default_value = "fates.yaml")]
        path: String,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
    /// Reel a group's log file (copy + truncate, keeps N backups)
    #[command(visible_alias = "rotate")]
    Reel {
        /// The name of the process group
        name: String,

        /// Number of log backups to keep
        #[arg(long, short, default_value_t = 5)]
        keep: usize,
    },
    /// Stop a process group if running, then spin it up fresh
    #[command(visible_alias = "restart")]
    Respin {
        /// The name of the process group (omit with --all)
        #[arg(required_unless_present = "all")]
        name: Option<String>,

        /// Respin every registered process group, in dependency order
        #[arg(long)]
        all: bool,

        /// Skip graceful SIGTERM and force SIGKILL during the stop phase
        #[arg(long)]
        force: bool,
    },
}
