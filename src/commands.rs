use crate::config::Config;
use crate::error::Error;
use crate::shell;
use crate::state::{ProcessState, State, StateLock, pid_is_stale};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use sysinfo::System;

/// Register a group into state from its `fates.yaml` definition (cmd, cwd,
/// dependency-aware). Returns the command string for display. The stored args
/// are fully expanded (env + tilde), so downstream PID verification against
/// `/proc/<pid>/cmdline` matches what was actually spawned.
fn register_from_config(state: &mut State, config: &Config, name: &str) -> Result<String, Error> {
    let group = config
        .groups
        .get(name)
        .ok_or_else(|| Error::command(format!("Group '{}' not found in config", name)))?;
    let args: Vec<String> = shell::split(&group.cmd)
        .into_iter()
        .map(|t| shell::expand_env(&t, &group.env))
        .map(|t| shell::expand_tilde(&t))
        .collect();
    let cwd = group
        .cwd
        .clone()
        .map(|c| shell::expand_env(&c, &group.env))
        .map(|c| shell::expand_tilde(&c));
    state.processes.insert(
        name.to_string(),
        ProcessState {
            pid: None,
            cmd: group.cmd.clone(),
            args,
            cwd,
        },
    );
    Ok(group.cmd.clone())
}

pub fn spin(
    name: String,
    cmd: Vec<String>,
    config_path: &str,
    state_dir: &Path,
) -> Result<(), Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let mut state = State::load(state_dir);
    let config = Config::load(config_path)?;

    // Validate config if a file exists
    if std::path::Path::new(config_path).exists() {
        let errors = config.validate();
        if !errors.is_empty() {
            let mut msg = format!("Config validation errors in '{}':", config_path);
            for e in &errors {
                msg.push_str(&format!("\n  • {}", e));
            }
            return Err(Error::command(msg));
        }
    }

    let command_str = if !cmd.is_empty() {
        let command_str = cmd.join(" ");
        let empty_env = std::collections::HashMap::new();
        let args: Vec<String> = cmd
            .iter()
            .map(|t| shell::expand_env(t, &empty_env))
            .map(|t| shell::expand_tilde(&t))
            .collect();
        let cwd = config
            .groups
            .get(&name)
            .and_then(|g| g.cwd.clone())
            .map(|c| shell::expand_env(&c, &empty_env))
            .map(|c| shell::expand_tilde(&c));
        state.processes.insert(
            name.clone(),
            ProcessState {
                pid: None,
                cmd: command_str.clone(),
                args,
                cwd,
            },
        );
        command_str
    } else {
        register_from_config(&mut state, &config, &name)?
    };

    state.save(state_dir)?;
    println!("Spun thread '{}': {}", name, command_str);
    Ok(())
}

pub fn draw(name: Option<String>, config_path: &str, state_dir: &Path) -> Result<(), Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let config = Config::load(config_path)?;
    let mut state = State::load(state_dir);

    let targets: Vec<String> = match name {
        Some(n) => vec![n],
        None => {
            // Auto-register config groups that were never spun, so `draw --all`
            // can bring up the whole stack from fates.yaml alone.
            let errors = config.validate();
            if !errors.is_empty() {
                let mut msg = format!("Config validation errors in '{}':", config_path);
                for e in &errors {
                    msg.push_str(&format!("\n  • {}", e));
                }
                return Err(Error::command(msg));
            }
            for group_name in config.groups.keys() {
                if !state.processes.contains_key(group_name) {
                    register_from_config(&mut state, &config, group_name)?;
                    println!("Note: registered '{}' from config.", group_name);
                }
            }
            let names: Vec<String> = state.processes.keys().cloned().collect();
            config.topological_order(&names)
        }
    };

    let mut first_error: Option<Error> = None;
    for n in &targets {
        if let Err(e) = draw_one(n, &config, &mut state, state_dir) {
            first_error.get_or_insert(e);
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The command to execute for a group: the stored args if present, otherwise
/// tokenize the command string (for state files written by older versions).
fn effective_args(process_state: &ProcessState) -> Vec<String> {
    if !process_state.args.is_empty() {
        process_state.args.clone()
    } else {
        let empty_env = std::collections::HashMap::new();
        shell::split(&process_state.cmd)
            .into_iter()
            .map(|t| shell::expand_env(&t, &empty_env))
            .map(|t| shell::expand_tilde(&t))
            .collect()
    }
}

fn draw_one(name: &str, config: &Config, state: &mut State, state_dir: &Path) -> Result<(), Error> {
    let process_state = match state.processes.get_mut(name) {
        Some(ps) => ps,
        None => {
            return Err(Error::command(format!(
                "Group '{}' not spun. Run `fates spin {}` first.",
                name, name
            )));
        }
    };

    let args = effective_args(process_state);

    if args.is_empty() {
        return Err(Error::command(format!("Empty command for '{}'", name)));
    }

    // Only refuse to start when the recorded PID is still alive *and* still
    // matches the command. A stale PID (process dead, or the PID was reused by
    // an unrelated process) is cleared so we can start fresh.
    if let Some(pid) = process_state.pid {
        let alive = signal::kill(Pid::from_raw(pid as i32), None).is_ok();
        if alive && !pid_is_stale(pid, &args) {
            return Err(Error::command(format!(
                "Process '{}' is already running with PID {}",
                name, pid
            )));
        }
        if alive && pid_is_stale(pid, &args) {
            eprintln!(
                "Note: PID {} no longer matches '{}' — treating as stopped.",
                pid, name
            );
        }
        process_state.pid = None;
    }

    // Await readiness of dependencies that have health checks configured
    if let Some(group) = config.groups.get(name) {
        for dep in group.depends_names() {
            if let Some(dep_group) = config.groups.get(&dep) {
                if group.requires_healthy(&dep) {
                    if let Some(ref hc) = dep_group.healthcheck {
                        println!("Awaiting healthy status for dependency '{}'...", dep);
                        crate::health::wait_for_healthy(&dep, hc)?;
                    }
                }
            }
        }
    }

    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..]);

    // Prefer cwd from config if available, otherwise fall back to state
    let cwd = config
        .groups
        .get(name)
        .and_then(|g| g.cwd.clone())
        .or_else(|| process_state.cwd.clone())
        .map(|c| shell::expand_tilde(&c));
    if let Some(cwd) = &cwd {
        cmd.current_dir(cwd);
    }

    // Apply the group's `env:` block to the spawned process.
    if let Some(group) = config.groups.get(name) {
        for (k, v) in &group.env {
            cmd.env(k, v);
        }
    }

    let _ = std::fs::create_dir_all(state_dir);
    let log_path = State::log_file(state_dir, name);

    let (stdout_file, stderr_file) = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            let err_file = file.try_clone().ok();
            (
                Stdio::from(file),
                err_file.map(Stdio::from).unwrap_or_else(Stdio::null),
            )
        }
        Err(e) => {
            eprintln!("Warning: Could not open log file, using null: {}", e);
            (Stdio::null(), Stdio::null())
        }
    };

    cmd.stdin(Stdio::null())
        .stdout(stdout_file)
        .stderr(stderr_file);

    // Put the process into its own process group
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setpgid(nix::unistd::Pid::from_raw(0), nix::unistd::Pid::from_raw(0))
                .map_err(std::io::Error::other)?;
            Ok(())
        });
    }

    println!("Drawing thread '{}' in background...", name);

    let child = cmd
        .spawn()
        .map_err(|e| Error::system(format!("Failed to spawn process '{}': {}", name, e)))?;
    let pid = child.id();
    process_state.pid = Some(pid);
    if let Err(e) = state.save(state_dir) {
        // Don't leave an untracked orphan behind: kill what we just started.
        let _ = signal::kill(Pid::from_raw(-(pid as i32)), Signal::SIGKILL);
        return Err(e);
    }
    println!(
        "Thread '{}' drawing. PID: {}. Logs: {}",
        name,
        pid,
        log_path.display()
    );

    if let Some(group) = config.groups.get(name) {
        if let Some(ref hc) = group.healthcheck {
            println!("Performing initial health check for thread '{}'...", name);
            if let Err(e) = crate::health::wait_for_healthy(name, hc) {
                eprintln!("Warning: Initial health check for '{}' failed: {}", name, e);
            }
        }
    }
    Ok(())
}

pub fn cut(
    name: Option<String>,
    force: bool,
    config_path: &str,
    state_dir: &Path,
) -> Result<(), Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let config = Config::load(config_path)?;
    let mut state = State::load(state_dir);

    let all = name.is_none();
    let targets: Vec<String> = match name {
        Some(n) => vec![n],
        None => {
            let names: Vec<String> = state.processes.keys().cloned().collect();
            let mut order = config.topological_order(&names);
            order.reverse();
            order
        }
    };

    let mut first_error: Option<Error> = None;
    for n in &targets {
        if let Err(e) = cut_one(n, force, all, &mut state, state_dir) {
            first_error.get_or_insert(e);
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn cut_one(
    name: &str,
    force: bool,
    skip_if_stopped: bool,
    state: &mut State,
    state_dir: &Path,
) -> Result<(), Error> {
    let process_state = match state.processes.get_mut(name) {
        Some(ps) => ps,
        None => {
            return Err(Error::command(format!(
                "Group '{}' not found in state.",
                name
            )));
        }
    };

    let Some(pid) = process_state.pid else {
        if skip_if_stopped {
            println!("Thread '{}' is not running, skipping.", name);
            return Ok(());
        }
        return Err(Error::command(format!("Thread '{}' is not running.", name)));
    };

    // Never signal a PID that no longer belongs to this command: the OS may
    // have reused it for an unrelated process, and a negative-PID kill would
    // hit that process's whole group.
    if pid_is_stale(pid, &effective_args(process_state)) {
        process_state.pid = None;
        state.save(state_dir)?;
        if skip_if_stopped {
            println!(
                "Thread '{}' already exited (cleared stale PID {}), skipping.",
                name, pid
            );
            return Ok(());
        }
        return Err(Error::command(format!(
            "Thread '{}' is not running (stale PID {} no longer matches the command).",
            name, pid
        )));
    }

    // Negative PID to kill the entire process group
    let p = Pid::from_raw(-(pid as i32));

    println!("Cutting thread '{}' (PID {})...", name, pid);

    if force {
        // Immediately SIGKILL
        println!("  Sending SIGKILL (--force)...");
        match signal::kill(p, Signal::SIGKILL) {
            Ok(_) => {
                process_state.pid = None;
                state.save(state_dir)?;
                println!("Thread '{}' cut (forced).", name);
            }
            Err(e) => {
                process_state.pid = None;
                state.save(state_dir)?;
                return Err(Error::system(format!(
                    "Failed to send SIGKILL to PID {}: {}",
                    pid, e
                )));
            }
        }
    } else {
        // Graceful: SIGTERM first, wait up to 5s, then SIGKILL
        println!("  Sending SIGTERM...");
        match signal::kill(p, Signal::SIGTERM) {
            Ok(_) => {
                let grace = Duration::from_secs(5);
                let step = Duration::from_millis(100);
                let mut elapsed = Duration::ZERO;
                let still_alive = loop {
                    thread::sleep(step);
                    elapsed += step;
                    // Check if the process group leader is still alive
                    if signal::kill(p, None).is_err() {
                        break false; // process gone
                    }
                    if elapsed >= grace {
                        break true; // still alive after grace
                    }
                };
                if still_alive {
                    println!("  Grace period elapsed. Sending SIGKILL...");
                    let _ = signal::kill(p, Signal::SIGKILL);
                }
                process_state.pid = None;
                state.save(state_dir)?;
                println!("Thread '{}' cut.", name);
            }
            Err(e) => {
                process_state.pid = None;
                state.save(state_dir)?;
                return Err(Error::system(format!(
                    "Failed to send SIGTERM to PID {}: {}",
                    pid, e
                )));
            }
        }
    }
    Ok(())
}

struct ProcInfo {
    status: &'static str,
    pid: Option<u32>,
    cpu: Option<f32>,
    memory_bytes: Option<u64>,
    uptime_seconds: Option<u64>,
}

/// Gather live process info for a group, clearing stale PIDs. Shared by the
/// `loom` table and `loom --json` views.
fn collect_proc_info(
    proc_state: &mut ProcessState,
    sys: &System,
    state_changed: &mut bool,
) -> ProcInfo {
    let mut status = "STOPPED";
    let mut pid = None;
    let mut cpu = None;
    let mut memory_bytes = None;
    let mut uptime_seconds = None;

    if let Some(p) = proc_state.pid {
        let sys_pid = sysinfo::Pid::from_u32(p);
        if let Some(proc) = sys.process(sys_pid) {
            if pid_is_stale(p, &effective_args(proc_state)) {
                proc_state.pid = None;
                *state_changed = true;
            } else {
                status = "RUNNING";
                pid = Some(p);
                cpu = Some(proc.cpu_usage());
                memory_bytes = Some(proc.memory());
                uptime_seconds = Some(proc.run_time());
            }
        } else {
            proc_state.pid = None;
            *state_changed = true;
        }
    }

    ProcInfo {
        status,
        pid,
        cpu,
        memory_bytes,
        uptime_seconds,
    }
}

pub fn loom(json: bool, watch: Option<u64>, state_dir: &Path) -> Result<(), Error> {
    match watch {
        Some(interval) => {
            let interval = Duration::from_millis(interval.saturating_mul(1000));
            if json {
                // JSON watch: just re-print the payload every interval.
                loop {
                    loom_once(json, state_dir)?;
                    thread::sleep(interval);
                }
            } else if std::io::stdout().is_terminal() {
                crate::dashboard::run_watch(interval, state_dir)
            } else {
                // Not a terminal (pipe, script, test): clear + re-print the
                // plain dashboard so the loop still refreshes visibly.
                loop {
                    use std::io::Write;
                    print!("\x1b[2J\x1b[H");
                    loom_once(json, state_dir)?;
                    std::io::stdout().flush().ok();
                    thread::sleep(interval);
                }
            }
        }
        None => loom_once(json, state_dir),
    }
}

/// Collect live rows and totals for the dashboard, clearing stale PIDs. Empty
/// state returns `None`. Shared by the table, `--json`, and interactive watch.
pub(crate) fn loom_rows(
    state_dir: &Path,
) -> Result<Option<(Vec<crate::dashboard::Row>, crate::dashboard::Totals, bool)>, Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let mut state = State::load(state_dir);
    if state.processes.is_empty() {
        return Ok(None);
    }

    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    thread::sleep(Duration::from_millis(50));
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut state_changed = false;
    let mut names: Vec<String> = state.processes.keys().cloned().collect();
    names.sort();

    let mut rows = Vec::new();
    let mut totals = crate::dashboard::Totals::default();
    for name in &names {
        let proc_state = state.processes.get_mut(name).unwrap();
        let info = collect_proc_info(proc_state, &sys, &mut state_changed);
        let running = info.status == "RUNNING";
        if running {
            totals.running += 1;
            totals.total_cpu += info.cpu.unwrap_or(0.0);
            totals.total_memory_bytes += info.memory_bytes.unwrap_or(0);
        }
        rows.push(crate::dashboard::Row {
            name: name.clone(),
            running,
            pid: info.pid,
            cpu: info.cpu,
            memory_bytes: info.memory_bytes,
            uptime_seconds: info.uptime_seconds,
            cmd: proc_state.cmd.clone(),
            cwd: proc_state.cwd.clone(),
        });
    }

    if state_changed {
        state.save(state_dir)?;
    }
    Ok(Some((rows, totals, state_changed)))
}

fn loom_once(json: bool, state_dir: &Path) -> Result<(), Error> {
    let Some((rows, totals, _)) = loom_rows(state_dir)? else {
        if json {
            println!("[]");
        } else {
            println!("No process groups found in state.");
        }
        return Ok(());
    };

    if json {
        let entries: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "status": if r.running { "RUNNING" } else { "STOPPED" },
                    "pid": r.pid,
                    "cmd": r.cmd,
                    "cwd": r.cwd,
                    "cpu_percent": r.cpu,
                    "memory_bytes": r.memory_bytes,
                    "uptime_seconds": r.uptime_seconds,
                })
            })
            .collect();
        let output = serde_json::to_string_pretty(&entries)
            .map_err(|e| Error::system(format!("Failed to serialize loom output: {}", e)))?;
        println!("{}", output);
    } else if std::io::stdout().is_terminal() {
        print!("{}", crate::dashboard::render_colored(&rows, &totals, None));
    } else {
        print!("{}", crate::dashboard::render_plain(&rows, &totals));
    }
    use std::io::Write;
    std::io::stdout().flush().ok();
    Ok(())
}

pub fn weave(name: String, config_path: &str) -> Result<(), Error> {
    let config = Config::load(config_path)?;

    if !config.groups.contains_key(&name) {
        return Err(Error::command(format!(
            "Group '{}' not found in '{}'",
            name, config_path
        )));
    }

    println!("{}", name);
    let mut visited = std::collections::HashSet::new();
    visited.insert(name.clone());

    if let Some(group) = config.groups.get(&name) {
        let deps = group.depends_names();
        let len = deps.len();
        for (i, dep) in deps.iter().enumerate() {
            let dep_is_last = i == len - 1;
            print_tree(dep, &config, "", dep_is_last, &mut visited);
        }
    }
    Ok(())
}

fn print_tree(
    name: &str,
    config: &Config,
    prefix: &str,
    is_last: bool,
    visited: &mut std::collections::HashSet<String>,
) {
    let marker = if is_last { "└── " } else { "├── " };
    println!("{}{}{}", prefix, marker, name);

    if visited.contains(name) {
        println!(
            "{}{} [CYCLE DETECTED]",
            prefix,
            if is_last { "    " } else { "│   " }
        );
        return;
    }
    visited.insert(name.to_string());

    if let Some(group) = config.groups.get(name) {
        let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
        let deps = group.depends_names();
        let len = deps.len();
        for (i, dep) in deps.iter().enumerate() {
            let dep_is_last = i == len - 1;
            print_tree(dep, config, &child_prefix, dep_is_last, visited);
        }
    }
    visited.remove(name);
}

pub fn omen(name: String, state_dir: &Path) -> Result<(), Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let mut state = State::load(state_dir);
    let proc_state = match state.processes.get_mut(&name) {
        Some(ps) => ps,
        None => {
            return Err(Error::command(format!(
                "Group '{}' not found in state.",
                name
            )));
        }
    };

    println!("========================================");
    println!(" OMEN: {}", name);
    println!("========================================");
    println!("Command:     {}", proc_state.cmd);
    println!(
        "CWD:         {}",
        proc_state.cwd.as_deref().unwrap_or("[Default]")
    );

    let mut state_changed = false;

    if let Some(pid) = proc_state.pid {
        let mut sys = System::new_all();
        sys.refresh_all();
        thread::sleep(Duration::from_millis(100));
        sys.refresh_all();

        let sys_pid = sysinfo::Pid::from_u32(pid);
        if let Some(proc) = sys.process(sys_pid) {
            if pid_is_stale(pid, &effective_args(proc_state)) {
                println!("Status:      STOPPED (Stale PID {})", pid);
                proc_state.pid = None;
                state_changed = true;
            } else {
                println!("Status:      RUNNING");
                println!("PID:         {}", pid);
                println!("CPU Usage:   {:.1}%", proc.cpu_usage());
                println!(
                    "Memory:      {:.1} MB ({} bytes)",
                    proc.memory() as f64 / 1024.0 / 1024.0,
                    proc.memory()
                );

                let secs = proc.run_time();
                let hours = secs / 3600;
                let mins = (secs % 3600) / 60;
                let secs = secs % 60;
                println!(
                    "Uptime:      {}h {}m {}s ({} seconds)",
                    hours,
                    mins,
                    secs,
                    proc.run_time()
                );

                if let Some(exe) = proc.exe() {
                    println!("Executable:  {}", exe.display());
                }
            }
        } else {
            println!("Status:      STOPPED (Stale PID {})", pid);
            proc_state.pid = None;
            state_changed = true;
        }
    } else {
        println!("Status:      STOPPED");
    }
    println!("========================================");

    if state_changed {
        state.save(state_dir)?;
    }
    Ok(())
}

/// Return the last `n` lines of the file, reading only from the end of the
/// file instead of loading the whole thing into memory. Uses a reverse scan
/// in chunks so `--tail` stays cheap even on very large logs.
pub(crate) fn tail_bytes(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    if n == 0 || len == 0 {
        return Ok(Vec::new());
    }

    const CHUNK: u64 = 8192;
    let mut pos = len;
    let mut newlines_seen = 0;
    let mut start = 0;
    // A file typically ends with '\n'; that trailing newline terminates the
    // last (possibly empty) line and is not counted as a line boundary.
    let mut skip_trailing_newline = true;

    'scan: loop {
        if pos == 0 {
            break;
        }
        let read_from = pos.saturating_sub(CHUNK);
        f.seek(SeekFrom::Start(read_from))?;
        let mut chunk = vec![0u8; (pos - read_from) as usize];
        f.read_exact(&mut chunk)?;

        for (i, &b) in chunk.iter().enumerate().rev() {
            if b != b'\n' {
                skip_trailing_newline = false;
                continue;
            }
            if skip_trailing_newline {
                skip_trailing_newline = false;
                continue;
            }
            newlines_seen += 1;
            if newlines_seen >= n {
                start = read_from + i as u64 + 1;
                break 'scan;
            }
        }
        pos = read_from;
    }

    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Follow the log file, printing newly appended bytes until interrupted.
/// Re-opens the file if it shrinks (rotation), so a rotated log is followed
/// seamlessly. When `tail` is given, the last `n` lines are printed first
/// (like `tail -f -n N`) before following from where they ended.
fn follow_log(log_path: &Path, tail: Option<usize>) -> Result<(), Error> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut f = std::fs::File::open(log_path).map_err(|e| {
        Error::system(format!(
            "Failed to open log file '{}': {}",
            log_path.display(),
            e
        ))
    })?;
    let mut offset = match tail {
        Some(0) => 0,
        Some(n) => {
            let buf = tail_bytes(log_path, n).map_err(|e| {
                Error::system(format!(
                    "Failed to read tail of log file '{}': {}",
                    log_path.display(),
                    e
                ))
            })?;
            print!("{}", String::from_utf8_lossy(&buf));
            std::io::stdout().flush().ok();
            std::fs::metadata(log_path)
                .map(|m| m.len())
                .unwrap_or(buf.len() as u64)
        }
        None => f.metadata().map(|m| m.len()).unwrap_or(0),
    };

    loop {
        let current_len = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
        if current_len < offset {
            // The log was rotated: reopen the new file and start from the top.
            f = std::fs::File::open(log_path).map_err(|e| {
                Error::system(format!(
                    "Failed to reopen rotated log file '{}': {}",
                    log_path.display(),
                    e
                ))
            })?;
            offset = 0;
        }

        f.seek(SeekFrom::Start(offset)).ok();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| {
            Error::system(format!(
                "Failed to read log file '{}': {}",
                log_path.display(),
                e
            ))
        })?;
        if !buf.is_empty() {
            print!("{}", String::from_utf8_lossy(&buf));
            std::io::stdout().flush().ok();
            offset += buf.len() as u64;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn follow_all_logs(state_dir: &Path, tail: Option<usize>) -> Result<(), Error> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let state = State::load(state_dir);
    let mut names: Vec<String> = state.processes.keys().cloned().collect();
    names.sort();

    if names.is_empty() {
        println!("No process groups found in state.");
        return Ok(());
    }

    let colors = ["36", "35", "32", "33", "34", "31"]; // Cyan, Magenta, Green, Yellow, Blue, Red

    struct Streamer {
        name: String,
        color: &'static str,
        file: Option<std::fs::File>,
        offset: u64,
        log_path: std::path::PathBuf,
        line_buf: String,
    }

    let mut streamers: Vec<Streamer> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let color = colors[i % colors.len()];
        let log_path = State::log_file(state_dir, name);
        let mut file = std::fs::File::open(&log_path).ok();
        let mut offset = 0;
        if let Some(ref mut f) = file {
            if let Some(n) = tail {
                if let Ok(buf) = tail_bytes(&log_path, n) {
                    let s = String::from_utf8_lossy(&buf);
                    for line in s.lines() {
                        println!("\x1b[{}m[{}]\x1b[0m {}", color, name, line);
                    }
                    offset = f.metadata().map(|m| m.len()).unwrap_or(0);
                }
            } else {
                offset = f.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        streamers.push(Streamer {
            name: name.clone(),
            color,
            file,
            offset,
            log_path,
            line_buf: String::new(),
        });
    }

    loop {
        let mut any_activity = false;
        for s in &mut streamers {
            if s.file.is_none() && s.log_path.exists() {
                s.file = std::fs::File::open(&s.log_path).ok();
                s.offset = 0;
            }

            if let Some(ref mut f) = s.file {
                let current_len = std::fs::metadata(&s.log_path).map(|m| m.len()).unwrap_or(0);
                if current_len < s.offset {
                    // Rotated
                    if let Ok(new_f) = std::fs::File::open(&s.log_path) {
                        *f = new_f;
                        s.offset = 0;
                    }
                }

                if f.seek(SeekFrom::Start(s.offset)).is_ok() {
                    let mut buf = Vec::new();
                    if f.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                        s.offset += buf.len() as u64;
                        any_activity = true;
                        let text = String::from_utf8_lossy(&buf);
                        s.line_buf.push_str(&text);

                        while let Some(idx) = s.line_buf.find('\n') {
                            let line = s.line_buf[..idx].to_string();
                            s.line_buf.drain(..=idx);
                            println!("\x1b[{}m[{}]\x1b[0m {}", s.color, s.name, line);
                            std::io::stdout().flush().ok();
                        }
                    }
                }
            }
        }

        if !any_activity {
            thread::sleep(Duration::from_millis(150));
        }
    }
}

fn print_all_logs(state_dir: &Path, tail: Option<usize>) -> Result<(), Error> {
    let state = State::load(state_dir);
    let mut names: Vec<String> = state.processes.keys().cloned().collect();
    names.sort();

    if names.is_empty() {
        println!("No process groups found in state.");
        return Ok(());
    }

    for name in &names {
        let log_path = State::log_file(state_dir, name);
        if !log_path.exists() {
            continue;
        }
        println!("==> {}.log <==", name);
        if let Some(n) = tail {
            if let Ok(buf) = tail_bytes(&log_path, n) {
                print!("{}", String::from_utf8_lossy(&buf));
                if !buf.is_empty() && !buf.ends_with(b"\n") {
                    println!();
                }
            }
        } else if let Ok(content) = std::fs::read_to_string(&log_path) {
            print!("{}", content);
            if !content.is_empty() && !content.ends_with('\n') {
                println!();
            }
        }
    }
    Ok(())
}

pub fn logs(
    name: Option<String>,
    all: bool,
    tail: Option<usize>,
    follow: bool,
    state_dir: &Path,
) -> Result<(), Error> {
    if all || name.is_none() {
        if follow {
            return follow_all_logs(state_dir, tail);
        } else {
            return print_all_logs(state_dir, tail);
        }
    }

    let name = name.unwrap();
    let log_path = State::log_file(state_dir, &name);

    if !log_path.exists() {
        return Err(Error::command(format!(
            "No log file found for '{}'. Has it been drawn yet? (expected: {})",
            name,
            log_path.display()
        )));
    }

    if follow {
        return follow_log(&log_path, tail);
    }

    if let Some(n) = tail {
        let buf = tail_bytes(&log_path, n).map_err(|e| {
            Error::system(format!(
                "Failed to read tail of log file '{}': {}",
                log_path.display(),
                e
            ))
        })?;
        print!("{}", String::from_utf8_lossy(&buf));
        if !buf.is_empty() && !buf.ends_with(b"\n") {
            println!();
        }
    } else {
        let content = std::fs::read_to_string(&log_path).map_err(|e| {
            Error::system(format!(
                "Failed to read log file '{}': {}",
                log_path.display(),
                e
            ))
        })?;
        print!("{}", content);
    }
    Ok(())
}

/// Reel a group's log file: copy the current contents to `<name>.log.1`,
/// shift older backups down by one, and truncate the live file.
///
/// Copy+truncate (rather than rename) is deliberate: running processes hold
/// the log open with `O_APPEND`, so after truncation their next write lands at
/// the new start of the file, keeping the live log in sync.
pub fn reel(name: String, keep: usize, state_dir: &Path) -> Result<(), Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let keep = keep.max(1);
    let log_path = State::log_file(state_dir, &name);

    if !log_path.exists() {
        return Err(Error::command(format!(
            "No log file found for '{}'. Has it been drawn yet? (expected: {})",
            name,
            log_path.display()
        )));
    }

    // Shift existing backups down: .log.1 -> .log.2, .log.2 -> .log.3, ...
    for i in (1..keep).rev() {
        let src = log_path.with_extension(format!("log.{}", i));
        let dst = log_path.with_extension(format!("log.{}", i + 1));
        if src.exists() {
            if dst.exists() {
                std::fs::remove_file(&dst).map_err(|e| {
                    Error::system(format!(
                        "Failed to remove old backup '{}': {}",
                        dst.display(),
                        e
                    ))
                })?;
            }
            std::fs::rename(&src, &dst).map_err(|e| {
                Error::system(format!(
                    "Failed to reel backup '{}' -> '{}': {}",
                    src.display(),
                    dst.display(),
                    e
                ))
            })?;
        }
    }

    // Archive the current contents and truncate the live file.
    if let Some(len) = std::fs::metadata(&log_path)
        .map(|m| m.len())
        .ok()
        .filter(|&l| l > 0)
    {
        let one = log_path.with_extension("log.1");
        std::fs::copy(&log_path, &one).map_err(|e| {
            Error::system(format!(
                "Failed to archive '{}' to '{}': {}",
                log_path.display(),
                one.display(),
                e
            ))
        })?;
        println!(
            "Archived {} ({} bytes) to {}",
            log_path.display(),
            len,
            one.display()
        );
    }

    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&log_path)
        .map_err(|e| {
            Error::system(format!(
                "Failed to open '{}' for truncation: {}",
                log_path.display(),
                e
            ))
        })?;
    f.set_len(0).map_err(|e| {
        Error::system(format!(
            "Failed to truncate '{}': {}",
            log_path.display(),
            e
        ))
    })?;

    println!("Reeled log for '{}' (keeping {} backup(s)).", name, keep);
    Ok(())
}

/// Block until the given group's process has exited. A stopped group (or one
/// that already exited) returns immediately. Polls `/proc/<pid>/cmdline` so a
/// reused PID is never mistaken for the original process.
pub fn wait(name: String, timeout: Option<u64>, state_dir: &Path) -> Result<(), Error> {
    let (pid, args) = {
        let _lock = StateLock::acquire(state_dir)?;
        let state = State::load(state_dir);
        let proc_state = state
            .processes
            .get(&name)
            .ok_or_else(|| Error::command(format!("Group '{}' not found in state.", name)))?;
        (proc_state.pid, effective_args(proc_state))
    };

    let Some(pid) = pid else {
        return Ok(());
    };

    let deadline = timeout.map(|t| std::time::Instant::now() + Duration::from_secs(t));
    loop {
        if pid_is_stale(pid, &args) {
            return Ok(());
        }
        if deadline.is_some_and(|dl| std::time::Instant::now() >= dl) {
            return Err(Error::command(format!(
                "Timed out waiting for '{}' to exit after {}s.",
                name,
                timeout.unwrap_or(0)
            )));
        }
        thread::sleep(Duration::from_millis(200));
    }
}

const STARTER_CONFIG: &str = r#"# FATES starter config
#
# Bring up the whole stack with:
#   fates draw --all
#
# Per-group options:
#   cmd:      shell command to run (required)
#   cwd:      working directory (optional)
#   depends:  groups to start first (optional)
#   env:      environment variables for the process (optional)

groups:
  web:
    cmd: python3 -m http.server 8000
    cwd: ~

  # db:
  #   cmd: postgres -D ~/data/pgdata
  #   depends: [web]
  #   env:
  #     PGDATA: ~/data/pgdata
"#;

/// Write a starter `fates.yaml` template. Refuses to overwrite an existing
/// file.
pub fn init(path: &str) -> Result<(), Error> {
    let target = std::path::Path::new(path);
    if target.exists() {
        return Err(Error::command(format!(
            "'{}' already exists. Refusing to overwrite.",
            path
        )));
    }
    std::fs::write(target, STARTER_CONFIG)
        .map_err(|e| Error::system(format!("Failed to write '{}': {}", path, e)))?;
    println!("Wrote starter config to '{}'.", path);
    Ok(())
}

/// Stop a group if it is running, otherwise leave it alone. Unlike `cut`, a
/// stopped group is not an error here: respin draws a fresh instance either
/// way. Stale PIDs are cleared so the draw phase can start cleanly.
fn stop_if_running(
    name: &str,
    force: bool,
    state: &mut State,
    state_dir: &Path,
) -> Result<(), Error> {
    let pid = match state.processes.get(name) {
        Some(ps) => match ps.pid {
            Some(pid) => pid,
            None => return Ok(()),
        },
        None => return Ok(()),
    };

    let stale = {
        let ps = state.processes.get(name).unwrap();
        pid_is_stale(pid, &effective_args(ps))
    };

    if stale {
        state.processes.get_mut(name).unwrap().pid = None;
        state.save(state_dir)?;
        return Ok(());
    }

    cut_one(name, force, true, state, state_dir)
}

pub fn respin(
    name: Option<String>,
    force: bool,
    config_path: &str,
    state_dir: &Path,
) -> Result<(), Error> {
    let _lock = StateLock::acquire(state_dir)?;
    let config = Config::load(config_path)?;
    let mut state = State::load(state_dir);

    let targets: Vec<String> = match name {
        Some(n) => vec![n],
        None => {
            let names: Vec<String> = state.processes.keys().cloned().collect();
            config.topological_order(&names)
        }
    };

    let mut first_error: Option<Error> = None;
    for n in &targets {
        if let Err(e) = stop_if_running(n, force, &mut state, state_dir) {
            first_error.get_or_insert(e);
            continue;
        }
        if let Err(e) = draw_one(n, &config, &mut state, state_dir) {
            first_error.get_or_insert(e);
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
