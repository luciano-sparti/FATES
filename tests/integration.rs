//! Integration tests for the `fates` binary.
//!
//! Run with:  cargo test
//!
//! These tests call the binary at `target/debug/fates` directly, so make sure
//! you have run `cargo build` at least once before running them.
//!
//! Each test gets its own isolated state directory via `FATES_STATE_DIR`, so
//! tests are safe to run in parallel and never touch the real `/tmp/fates`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Absolute path to the compiled debug binary.
fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/fates")
}

/// An isolated state directory for one test, with a helper to run `fates`.
struct TestEnv {
    dir: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        TestEnv { dir: tempdir() }
    }

    /// Run `fates` with the given args and return (stdout, stderr, exit_code).
    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let output = Command::new(bin())
            .env("FATES_STATE_DIR", self.dir.as_path())
            .args(args)
            .output()
            .expect("Failed to execute fates binary. Have you run `cargo build`?");
        (
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
            output.status.code().unwrap_or(-1),
        )
    }

    /// Write a fates.yaml into the isolated dir and return its path.
    fn config(&self, content: &str) -> PathBuf {
        let path = self.dir.join("fates.yaml");
        fs::write(&path, content).expect("Failed to write temp config");
        path
    }

    /// Read a service's captured log from the isolated dir.
    fn log(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join(format!("{}.log", name))).unwrap_or_default()
    }

    /// Read a group's recorded PID from the state file.
    fn pid_of(&self, name: &str) -> Option<u32> {
        let raw = fs::read_to_string(self.dir.join("state.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        v["processes"][name]["pid"].as_u64().map(|p| p as u32)
    }
}

// ─── help & version ────────────────────────────────────────────────────────

#[test]
fn help_exits_successfully() {
    let env = TestEnv::new();
    let (stdout, _, code) = env.run(&["--help"]);
    assert_eq!(code, 0, "Expected exit 0 for --help");
    assert!(stdout.contains("fates"), "Expected 'fates' in help output");
    assert!(stdout.contains("spin"), "Expected 'spin' in help output");
    assert!(stdout.contains("loom"), "Expected 'loom' in help output");
}

#[test]
fn help_lists_aliases() {
    let env = TestEnv::new();
    let (stdout, _, code) = env.run(&["--help"]);
    assert_eq!(code, 0);
    for (cmd, alias) in [
        ("spin", "register"),
        ("draw", "start"),
        ("cut", "stop"),
        ("loom", "status"),
        ("weave", "tree"),
        ("omen", "info"),
        ("logs", "log"),
        ("reel", "rotate"),
        ("respin", "restart"),
    ] {
        let expected = format!("[aliases: {}]", alias);
        // clap renders `cmd ... [aliases: alias]`; just require both tokens near each other
        assert!(
            stdout.contains(&format!("{} ", cmd)) && stdout.contains(&expected),
            "help should show alias '{}' for '{}'. Output: {}",
            alias,
            cmd,
            stdout
        );
    }
}

#[test]
fn version_exits_successfully() {
    let env = TestEnv::new();
    let (stdout, _, code) = env.run(&["--version"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("fates"),
        "Expected 'fates' in version output"
    );
}

// ─── spin ──────────────────────────────────────────────────────────────────

#[test]
fn spin_registers_group_from_cli() {
    let env = TestEnv::new();
    let (stdout, stderr, code) = env.run(&["spin", "_test_spin_cli", "--", "echo", "hello"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stdout.contains("_test_spin_cli"),
        "Expected group name in output"
    );
}

#[test]
fn spin_from_config_file() {
    let env = TestEnv::new();
    let cfg = env.config("groups:\n  _test_cfg_spin:\n    cmd: echo from_config\n");
    let cfg_str = cfg.to_string_lossy();
    let (stdout, stderr, code) = env.run(&["--config", &cfg_str, "spin", "_test_cfg_spin"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(
        stdout.contains("_test_cfg_spin"),
        "Expected group name in spin output"
    );
}

#[test]
fn spin_unknown_group_without_cmd_exits_nonzero() {
    let env = TestEnv::new();
    let cfg = env.config("groups: {}\n");
    let cfg_str = cfg.to_string_lossy();
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "spin", "_nonexistent_group"]);
    assert_ne!(code, 0, "Expected non-zero exit for unknown group");
    assert!(
        stderr.contains("not found"),
        "Expected 'not found' in stderr: {}",
        stderr
    );
}

// ─── quoting ───────────────────────────────────────────────────────────────

#[test]
fn config_quoted_command_preserves_arguments() {
    let env = TestEnv::new();
    let cfg = env.config("groups:\n  q:\n    cmd: echo \"hello world\"\n");
    let cfg_str = cfg.to_string_lossy();

    let (_, stderr, code) = env.run(&["--config", &cfg_str, "spin", "q"]);
    assert_eq!(code, 0, "spin failed: {}", stderr);
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "draw", "q"]);
    assert_eq!(code, 0, "draw failed: {}", stderr);
    thread::sleep(Duration::from_millis(400));

    let log = env.log("q");
    assert!(
        log.contains("hello world"),
        "Expected quoted arg as one word: {:?}",
        log
    );
}

#[test]
fn inline_command_with_quotes_preserves_arguments() {
    let env = TestEnv::new();
    let (_, stderr, code) = env.run(&["spin", "q", "--", "echo", "hello world"]);
    assert_eq!(code, 0, "spin failed: {}", stderr);
    let (_, stderr, code) = env.run(&["draw", "q"]);
    assert_eq!(code, 0, "draw failed: {}", stderr);
    thread::sleep(Duration::from_millis(400));

    let log = env.log("q");
    assert!(
        log.contains("hello world"),
        "Expected quoted arg as one word: {:?}",
        log
    );
}

// ─── draw & cut ────────────────────────────────────────────────────────────

#[test]
fn draw_and_cut_a_process() {
    let env = TestEnv::new();
    let group = "_test_draw_cut";

    // Spin
    let (_, stderr, code) = env.run(&["spin", group, "--", "sleep", "60"]);
    assert_eq!(code, 0, "spin failed: {}", stderr);

    // Draw (detached)
    let (stdout, stderr, code) = env.run(&["draw", group]);
    assert_eq!(code, 0, "draw failed: {}", stderr);
    assert!(
        stdout.contains("PID") || stdout.contains("drawing"),
        "Unexpected draw output: {}",
        stdout
    );

    // Brief pause for the process to start
    thread::sleep(Duration::from_millis(300));

    // Loom should show it as RUNNING
    let (loom_out, _, _) = env.run(&["loom"]);
    assert!(loom_out.contains(group), "Expected group in loom output");
    assert!(
        loom_out.contains("RUNNING"),
        "Expected RUNNING status in loom output"
    );

    // Cut gracefully
    let (stdout, stderr, code) = env.run(&["cut", group]);
    assert_eq!(code, 0, "cut failed: {}", stderr);
    assert!(
        stdout.contains("cut"),
        "Expected 'cut' in output: {}",
        stdout
    );
}

#[test]
fn draw_already_running_is_rejected() {
    let env = TestEnv::new();
    let group = "_test_already_running";

    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));

    // Second draw should refuse
    let (_, stderr, code) = env.run(&["draw", group]);
    assert_ne!(code, 0, "Expected non-zero exit when already running");
    assert!(
        stderr.contains("already running"),
        "Expected 'already running' in stderr: {}",
        stderr
    );

    env.run(&["cut", "--force", group]);
}

#[test]
fn cut_force_terminates_immediately() {
    let env = TestEnv::new();
    let group = "_test_force_cut";

    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));

    let (stdout, stderr, code) = env.run(&["cut", "--force", group]);
    assert_eq!(code, 0, "force cut failed: {}", stderr);
    assert!(
        stdout.contains("forced") || stdout.contains("cut"),
        "Unexpected output: {}",
        stdout
    );
}

#[test]
fn cut_nonexistent_group_exits_with_error() {
    let env = TestEnv::new();
    let (_, stderr, code) = env.run(&["cut", "_no_such_group_xyz"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("not found"),
        "Expected 'not found': {}",
        stderr
    );
}

// ─── --all ─────────────────────────────────────────────────────────────────

#[test]
fn draw_all_starts_in_dependency_order() {
    let env = TestEnv::new();
    let cfg = env.config(
        "groups:\n  db:\n    cmd: sleep 60\n  api:\n    cmd: sleep 60\n    depends: [db]\n  web:\n    cmd: sleep 60\n    depends: [api]\n",
    );
    let cfg_str = cfg.to_string_lossy();

    for g in ["db", "api", "web"] {
        let (_, stderr, code) = env.run(&["--config", &cfg_str, "spin", g]);
        assert_eq!(code, 0, "spin {} failed: {}", g, stderr);
    }

    let (stdout, stderr, code) = env.run(&["--config", &cfg_str, "draw", "--all"]);
    assert_eq!(code, 0, "draw --all failed: {}", stderr);

    let pos = |needle: &str| {
        stdout
            .find(needle)
            .unwrap_or_else(|| panic!("Expected '{}' in draw --all output: {}", needle, stdout))
    };
    assert!(
        pos("Thread 'db'") < pos("Thread 'api'"),
        "db must start before api: {}",
        stdout
    );
    assert!(
        pos("Thread 'api'") < pos("Thread 'web'"),
        "api must start before web: {}",
        stdout
    );
}

#[test]
fn cut_all_stops_dependents_first() {
    let env = TestEnv::new();
    let cfg = env.config(
        "groups:\n  db:\n    cmd: sleep 60\n  api:\n    cmd: sleep 60\n    depends: [db]\n  web:\n    cmd: sleep 60\n    depends: [api]\n",
    );
    let cfg_str = cfg.to_string_lossy();

    for g in ["db", "api", "web"] {
        env.run(&["--config", &cfg_str, "spin", g]);
    }
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "draw", "--all"]);
    assert_eq!(code, 0, "draw --all failed: {}", stderr);

    let (stdout, stderr, code) = env.run(&["--config", &cfg_str, "cut", "--all"]);
    assert_eq!(code, 0, "cut --all failed: {}", stderr);

    let pos = |needle: &str| {
        stdout
            .find(needle)
            .unwrap_or_else(|| panic!("Expected '{}' in cut --all output: {}", needle, stdout))
    };
    assert!(
        pos("web") < pos("api"),
        "web must stop before api: {}",
        stdout
    );
    assert!(
        pos("api") < pos("db"),
        "api must stop before db: {}",
        stdout
    );
}

#[test]
fn cut_all_is_idempotent_when_groups_are_already_stopped() {
    let env = TestEnv::new();
    env.run(&["spin", "solo", "--", "sleep", "30"]);
    env.run(&["spin", "already_down", "--", "sleep", "30"]);

    let (_, stderr, code) = env.run(&["draw", "solo"]);
    assert_eq!(code, 0, "draw failed: {}", stderr);

    let (_, _stderr, code) = env.run(&["cut", "already_down"]);
    assert_ne!(code, 0, "cutting a stopped group by name should fail");

    let (stdout, stderr, code) = env.run(&["cut", "--all"]);
    assert_eq!(
        code, 0,
        "cut --all should tolerate already-stopped groups: {} {}",
        stderr, stdout
    );
    assert!(
        stdout.contains("not running, skipping"),
        "cut --all should note the skipped group: {}",
        stdout
    );
}

#[test]
fn draw_all_requires_no_config_for_state_only_groups() {
    let env = TestEnv::new();
    let cfg = env.config("groups: {}\n");
    let cfg_str = cfg.to_string_lossy();

    env.run(&["spin", "a", "--", "sleep", "60"]);
    env.run(&["spin", "b", "--", "sleep", "60"]);

    let (stdout, stderr, code) = env.run(&["--config", &cfg_str, "draw", "--all"]);
    assert_eq!(code, 0, "draw --all failed: {}", stderr);
    assert!(
        stdout.contains("Thread 'a'"),
        "Expected 'a' drawn: {}",
        stdout
    );
    assert!(
        stdout.contains("Thread 'b'"),
        "Expected 'b' drawn: {}",
        stdout
    );
}

// ─── stale PIDs ────────────────────────────────────────────────────────────

/// Kill a recorded PID externally (without telling fates), simulating the
/// process dying or its PID being reused.
fn kill_externally(env: &TestEnv, name: &str) -> u32 {
    let pid = env.pid_of(name).expect("expected a recorded PID");
    let status = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "external kill failed");
    thread::sleep(Duration::from_millis(200));
    pid
}

#[test]
fn loom_marks_dead_process_stopped() {
    let env = TestEnv::new();
    let group = "_test_dead_loom";

    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));
    assert!(env.pid_of(group).is_some(), "expected a PID after draw");

    kill_externally(&env, group);

    let (loom_out, _, _) = env.run(&["loom"]);
    assert!(
        loom_out.contains("STOPPED"),
        "Expected STOPPED in loom: {}",
        loom_out
    );
    assert!(
        env.pid_of(group).is_none(),
        "stale PID should be cleared from state after loom"
    );
}

#[test]
fn cut_stale_pid_is_rejected_without_signaling() {
    let env = TestEnv::new();
    let group = "_test_dead_cut";

    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));

    let old_pid = kill_externally(&env, group);

    let (_stdout, stderr, code) = env.run(&["cut", group]);
    assert_ne!(code, 0, "cutting a dead process should fail");
    assert!(
        stderr.contains("not running") && stderr.contains(&old_pid.to_string()),
        "expected stale-PID message: {}",
        stderr
    );
    assert!(
        env.pid_of(group).is_none(),
        "stale PID should be cleared from state after cut"
    );
}

#[test]
fn draw_respawns_after_dead_pid() {
    let env = TestEnv::new();
    let group = "_test_draw_respawn";

    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));
    let first_pid = env.pid_of(group).unwrap();

    kill_externally(&env, group);

    // draw must not refuse with "already running" — it should detect the
    // stale PID and start a fresh instance.
    let (_, stderr, code) = env.run(&["draw", group]);
    assert_eq!(code, 0, "draw should respawn a dead process: {}", stderr);
    thread::sleep(Duration::from_millis(300));
    let second_pid = env.pid_of(group).expect("expected a new PID after respawn");
    assert_ne!(first_pid, second_pid, "expected a fresh PID after respawn");
}

#[test]
fn draw_all_registers_config_groups_automatically() {
    let env = TestEnv::new();
    let cfg = env.config(
        "groups:\n  db:\n    cmd: sleep 60\n  web:\n    cmd: sleep 60\n    depends: [db]\n",
    );
    let cfg_str = cfg.to_string_lossy();

    // No spin: draw --all must register both groups from config and start them.
    let (stdout, stderr, code) = env.run(&["--config", &cfg_str, "draw", "--all"]);
    assert_eq!(code, 0, "draw --all failed: {}", stderr);
    assert!(
        stdout.contains("registered 'db'") && stdout.contains("registered 'web'"),
        "expected auto-registration notes: {}",
        stdout
    );
    thread::sleep(Duration::from_millis(300));

    let raw = fs::read_to_string(env.dir.join("state.json")).unwrap_or_default();
    assert!(
        raw.contains("\"db\"") && raw.contains("\"web\""),
        "both groups should be persisted in state: {}",
        raw
    );
    let v: serde_json::Value = serde_json::from_str(&raw).expect("valid state json");
    assert!(
        v["processes"]["db"]["pid"].is_u64() && v["processes"]["web"]["pid"].is_u64(),
        "both groups should be running: {}",
        raw
    );
}

// ─── concurrency ───────────────────────────────────────────────────────────

#[test]
fn concurrent_spins_preserve_all_groups() {
    let env = TestEnv::new();
    let n = 25;

    let mut children = Vec::new();
    for i in 0..n {
        let child = Command::new(bin())
            .env("FATES_STATE_DIR", env.dir.as_path())
            .args(["spin", &format!("conc_{i}"), "--", "echo", "x"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn fates");
        children.push(child);
    }
    for mut child in children {
        assert!(child.wait().unwrap().success(), "a concurrent spin failed");
    }

    let raw = fs::read_to_string(env.dir.join("state.json")).unwrap();
    for i in 0..n {
        assert!(
            raw.contains(&format!("conc_{i}")),
            "group conc_{} lost to a race:\n{}",
            i,
            raw
        );
    }
}

// ─── state dir override ────────────────────────────────────────────────────

#[test]
fn state_dir_flag_controls_state_location() {
    let env = TestEnv::new();
    let other = tempdir();

    let (_, stderr, code) = env.run(&[
        "--state-dir",
        other.as_path().to_str().unwrap(),
        "spin",
        "g",
        "--",
        "echo",
        "x",
    ]);
    assert_eq!(code, 0, "spin failed: {}", stderr);

    assert!(
        other.join("state.json").exists(),
        "state.json should be written to --state-dir"
    );
    assert!(
        !env.dir.join("state.json").exists(),
        "state.json must not be written to FATES_STATE_DIR when --state-dir is given"
    );
}

#[test]
fn system_errors_exit_with_code_two() {
    let env = TestEnv::new();
    // A file in the way of the state dir makes lock acquisition fail, which is
    // a system-level error (exit 2), distinct from command errors (exit 1).
    fs::write(env.dir.join("blocker"), b"x").unwrap();
    let bad_dir = format!("{}/blocker/sub", env.dir.display());

    let (_, stderr, code) = env.run(&["--state-dir", &bad_dir, "loom"]);
    assert_eq!(
        code, 2,
        "expected exit 2 for a system error, stderr: {}",
        stderr
    );
}

// ─── loom ──────────────────────────────────────────────────────────────────

#[test]
fn loom_prints_table_headers() {
    let env = TestEnv::new();
    env.run(&["spin", "_test_loom_header", "--", "echo", "x"]);

    let (stdout, _, _) = env.run(&["loom"]);
    assert!(stdout.contains("NAME"), "Missing NAME column");
    assert!(stdout.contains("STATUS"), "Missing STATUS column");
    assert!(stdout.contains("PID"), "Missing PID column");
}

#[test]
fn loom_prints_totals_row() {
    let env = TestEnv::new();
    env.run(&["spin", "_test_loom_totals", "--", "sleep", "30"]);
    let (_, stderr, code) = env.run(&["draw", "_test_loom_totals"]);
    assert_eq!(code, 0, "draw failed: {}", stderr);

    let (stdout, stderr, _) = env.run(&["loom"]);
    assert!(
        stdout.contains("running") && stdout.contains("CPU"),
        "loom should include a totals row, got: {} {}",
        stderr,
        stdout
    );
}

#[test]
fn loom_empty_state_shows_message() {
    let env = TestEnv::new();
    let (stdout, _, _) = env.run(&["loom"]);
    assert!(
        stdout.contains("No process groups"),
        "Expected empty state message: {}",
        stdout
    );
}

#[test]
fn loom_json_empty_state_is_empty_array() {
    let env = TestEnv::new();
    let (stdout, stderr, code) = env.run(&["loom", "--json"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert_eq!(
        stdout.trim(),
        "[]",
        "Expected empty JSON array, got: {}",
        stdout
    );
}

#[test]
fn loom_watch_refreshes_repeatedly() {
    use std::io::Read;

    let env = TestEnv::new();
    env.run(&["spin", "_test_loom_watch", "--", "sleep", "30"]);

    let mut child = Command::new(bin())
        .env("FATES_STATE_DIR", env.dir.as_path())
        .args(["loom", "--watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn `fates loom --watch`");

    thread::sleep(Duration::from_millis(900));
    let _ = child.kill();
    let _ = child.wait();

    let mut buf = String::new();
    let mut out = child.stdout.take().expect("no stdout pipe");
    let _ = out.read_to_string(&mut buf);

    assert!(
        buf.contains("NAME") && buf.contains("STATUS"),
        "watch should render the dashboard, got: {}",
        buf
    );
}

#[test]
fn loom_json_reports_running_process() {
    let env = TestEnv::new();
    let group = "_test_loom_json";
    env.run(&["spin", group, "--", "sleep", "30"]);

    let (_, stderr, code) = env.run(&["draw", group]);
    assert_eq!(code, 0, "draw failed: {}", stderr);

    let (stdout, stderr, code) = env.run(&["loom", "--json"]);
    assert_eq!(code, 0, "loom --json failed: {}", stderr);

    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("loom --json output must be valid JSON");
    let arr = v.as_array().expect("loom --json output must be an array");
    assert_eq!(arr.len(), 1, "Expected one entry, got: {}", stdout);

    let entry = &arr[0];
    assert_eq!(entry["name"], group, "Unexpected name in {}", stdout);
    assert_eq!(
        entry["status"], "RUNNING",
        "Expected RUNNING status in {}",
        stdout
    );
    assert!(entry["pid"].is_u64(), "Expected numeric pid in {}", stdout);
    assert_eq!(entry["cmd"], "sleep 30", "Unexpected cmd in {}", stdout);
    assert!(
        entry["uptime_seconds"].is_u64(),
        "Expected numeric uptime_seconds in {}",
        stdout
    );
    assert!(
        entry["cpu_percent"].is_f64() || entry["cpu_percent"].is_null(),
        "Expected numeric or null cpu_percent in {}",
        stdout
    );
    assert!(
        entry["memory_bytes"].is_u64() || entry["memory_bytes"].is_null(),
        "Expected numeric or null memory_bytes in {}",
        stdout
    );

    // --json must not mix in the human table
    assert!(
        !stdout.contains("STATUS"),
        "JSON output leaked table headers"
    );
}

// ─── weave ─────────────────────────────────────────────────────────────────

#[test]
fn weave_prints_dependency_tree() {
    let env = TestEnv::new();
    let cfg = env.config(
        "groups:\n  db:\n    cmd: postgres\n  web:\n    cmd: npm start\n    depends: [db]\n",
    );
    let cfg_str = cfg.to_string_lossy();
    let (stdout, _, code) = env.run(&["--config", &cfg_str, "weave", "web"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("web"), "Expected 'web' in tree");
    assert!(stdout.contains("db"), "Expected 'db' dependency in tree");
    assert!(
        stdout.contains("└──") || stdout.contains("\\--"),
        "Expected tree characters"
    );
}

#[test]
fn weave_unknown_group_exits_with_error() {
    let env = TestEnv::new();
    let cfg = env.config("groups: {}\n");
    let cfg_str = cfg.to_string_lossy();
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "weave", "_no_such_group"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("not found"),
        "Expected 'not found': {}",
        stderr
    );
}

// ─── logs ──────────────────────────────────────────────────────────────────

#[test]
fn logs_for_drawn_process_contains_output() {
    let env = TestEnv::new();
    let group = "_test_logs";

    env.run(&["spin", group, "--", "echo", "hello_from_fates"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(400));

    let (stdout, stderr, code) = env.run(&["logs", group]);
    assert_eq!(code, 0, "logs failed: {}", stderr);
    assert!(
        stdout.contains("hello_from_fates") || stdout.contains("Logs for"),
        "Expected log content: {}",
        stdout
    );
}

#[test]
fn logs_for_undrawn_process_shows_error() {
    let env = TestEnv::new();
    let (_, stderr, code) = env.run(&["logs", "_never_drawn_xyz"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("No log file"),
        "Expected 'No log file' error: {}",
        stderr
    );
}

#[test]
fn logs_tail_returns_only_last_lines() {
    let env = TestEnv::new();
    let group = "_test_logs_tail";
    env.run(&[
        "spin",
        group,
        "--",
        "sh",
        "-c",
        "printf 'line1\\nline2\\nline3\\nline4\\nline5\\n'",
    ]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(500));

    let (stdout, stderr, code) = env.run(&["logs", group, "--tail", "2"]);
    assert_eq!(code, 0, "logs --tail failed: {}", stderr);
    assert!(
        stdout.contains("line4") && stdout.contains("line5"),
        "Expected last 2 lines, got: {}",
        stdout
    );
    assert!(
        !stdout.contains("line1") && !stdout.contains("line2"),
        "Tail leaked earlier lines: {}",
        stdout
    );

    // Tail larger than the file returns everything.
    let (stdout, _, _) = env.run(&["logs", group, "--tail", "100"]);
    assert!(
        stdout.contains("line1") && stdout.contains("line5"),
        "Expected all lines when tail exceeds file size: {}",
        stdout
    );

    // Tail 0 returns nothing.
    let (stdout, _, code) = env.run(&["logs", group, "--tail", "0"]);
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "", "Expected empty output for --tail 0");
}

#[test]
fn logs_follow_streams_newly_appended_lines() {
    use std::io::Read;

    let env = TestEnv::new();
    let group = "_test_logs_follow";
    env.run(&[
        "spin",
        group,
        "--",
        "sh",
        "-c",
        "echo first; sleep 1; echo second; sleep 30",
    ]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(800));

    let mut child = Command::new(bin())
        .env("FATES_STATE_DIR", env.dir.as_path())
        .args(["logs", group, "--follow"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn `fates logs --follow`");

    // Wait long enough for 'second' to be appended and picked up by the poll.
    thread::sleep(Duration::from_millis(1600));
    let _ = child.kill();
    let _ = child.wait();

    let mut buf = String::new();
    let mut out = child.stdout.take().expect("no stdout pipe");
    let _ = out.read_to_string(&mut buf);

    assert!(
        buf.contains("second"),
        "Follow should stream newly appended lines, got: {}",
        buf
    );
    assert!(
        !buf.contains("first"),
        "Follow must only print new output, not history: {}",
        buf
    );
}

// ─── reel ────────────────────────────────────────────────────────────────

#[test]
fn reel_archives_log_and_starts_fresh() {
    let env = TestEnv::new();
    let group = "_test_reel";
    env.run(&[
        "spin",
        group,
        "--",
        "sh",
        "-c",
        "echo rotation_old; sleep 30",
    ]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(500));

    let (_, stderr, code) = env.run(&["reel", group]);
    assert_eq!(code, 0, "reel failed: {}", stderr);

    // The old content is archived to .log.1.
    let backup = fs::read_to_string(env.dir.join(format!("{}.log.1", group))).unwrap_or_default();
    assert!(
        backup.contains("rotation_old"),
        "Backup should hold old lines, got: {}",
        backup
    );

    // The live log is truncated and clean.
    let current = env.log(group);
    assert!(
        !current.contains("rotation_old"),
        "Live log should be empty after rotation, got: {}",
        current
    );

    // A second rotation shifts generations down.
    let (_, stderr, code) = env.run(&["reel", group]);
    assert_eq!(code, 0, "second reel failed: {}", stderr);
    let gen2 = fs::read_to_string(env.dir.join(format!("{}.log.2", group))).unwrap_or_default();
    assert!(
        gen2.contains("rotation_old"),
        "Expected first archive shifted to .log.2, got: {}",
        gen2
    );
}

#[test]
fn reel_unknown_group_shows_error() {
    let env = TestEnv::new();
    let (_, stderr, code) = env.run(&["reel", "_never_drawn_xyz"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("No log file"),
        "Expected 'No log file' error: {}",
        stderr
    );
}

#[test]
fn rotate_alias_behaves_like_reel() {
    let env = TestEnv::new();
    let group = "_test_rotate_alias";
    env.run(&["spin", group, "--", "sh", "-c", "echo alias_old; sleep 30"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(500));

    // `rotate` is an alias for `reel` and must archive + truncate the log.
    let (_, stderr, code) = env.run(&["rotate", group]);
    assert_eq!(code, 0, "rotate alias failed: {}", stderr);
    let backup = fs::read_to_string(env.dir.join(format!("{}.log.1", group))).unwrap_or_default();
    assert!(
        backup.contains("alias_old"),
        "alias should archive the log, got: {}",
        backup
    );
    assert!(
        !env.log(group).contains("alias_old"),
        "alias should truncate the live log"
    );
}

// ─── respin ───────────────────────────────────────────────────────────────

#[test]
fn respin_stops_and_starts_group() {
    let env = TestEnv::new();
    let group = "_test_respin";
    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));
    let pid1 = env.pid_of(group).expect("PID after first draw");

    let (_, stderr, code) = env.run(&["respin", group]);
    assert_eq!(code, 0, "respin failed: {}", stderr);
    thread::sleep(Duration::from_millis(300));

    let pid2 = env.pid_of(group).expect("PID after respin");
    assert_ne!(pid1, pid2, "respin must start a fresh process");
    assert!(
        env.log(group).contains("Thread") || env.log(group).is_empty(),
        "unexpected log content"
    );

    // The old process must actually be gone.
    let alive = Command::new("kill")
        .args(["-0", &pid1.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(!alive, "old process {} should be stopped", pid1);

    // And the new one is alive.
    let alive2 = Command::new("kill")
        .args(["-0", &pid2.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(alive2, "new process {} should be running", pid2);
}

#[test]
fn respin_of_stopped_group_starts_it() {
    let env = TestEnv::new();
    let group = "_test_respin_stopped";
    env.run(&["spin", group, "--", "sleep", "60"]);

    // Never drawn: respin should simply start it.
    let (_, stderr, code) = env.run(&["respin", group]);
    assert_eq!(code, 0, "respin of a stopped group failed: {}", stderr);
    thread::sleep(Duration::from_millis(300));
    assert!(
        env.pid_of(group).is_some(),
        "respin should have started the group"
    );
}

#[test]
fn respin_unknown_group_shows_error() {
    let env = TestEnv::new();
    let (_, stderr, code) = env.run(&["respin", "_never_spun_xyz"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("not spun"),
        "Expected 'not spun' error: {}",
        stderr
    );
}

#[test]
fn restart_alias_behaves_like_respin() {
    let env = TestEnv::new();
    let group = "_test_restart_alias";
    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));
    let pid1 = env.pid_of(group).expect("PID after first draw");

    // `restart` is an alias for `respin` and must stop + respin the group.
    let (_, stderr, code) = env.run(&["restart", group]);
    assert_eq!(code, 0, "restart alias failed: {}", stderr);
    thread::sleep(Duration::from_millis(300));

    let pid2 = env.pid_of(group).expect("PID after alias restart");
    assert_ne!(pid1, pid2, "alias restart must start a fresh process");
}

#[test]
fn logs_follow_with_tail_prints_history_first() {
    use std::io::Read;

    let env = TestEnv::new();
    let group = "_test_logs_follow_tail";
    env.run(&[
        "spin",
        group,
        "--",
        "sh",
        "-c",
        "printf 'hist1\\nhist2\\nhist3\\n'; sleep 30",
    ]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(600));

    let mut child = Command::new(bin())
        .env("FATES_STATE_DIR", env.dir.as_path())
        .args(["logs", group, "--follow", "--tail", "2"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn `fates logs --follow --tail`");

    // Give the follower time to print the history, then kill it.
    thread::sleep(Duration::from_millis(1200));
    let _ = child.kill();
    let _ = child.wait();

    let mut buf = String::new();
    let mut out = child.stdout.take().expect("no stdout pipe");
    let _ = out.read_to_string(&mut buf);

    assert!(
        buf.contains("hist2") && buf.contains("hist3"),
        "follow --tail should print the last lines first, got: {}",
        buf
    );
    assert!(
        !buf.contains("hist1"),
        "follow --tail 2 must not include the whole history: {}",
        buf
    );
}

#[test]
fn wait_times_out_on_long_running_process() {
    let env = TestEnv::new();
    let group = "_test_wait_timeout";
    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));

    let (_, stderr, code) = env.run(&["wait", group, "--timeout", "1"]);
    assert_ne!(code, 0, "wait should time out");
    assert!(
        stderr.contains("Timed out"),
        "Expected timeout message: {}",
        stderr
    );
}

#[test]
fn wait_returns_when_process_exits() {
    let env = TestEnv::new();
    let group = "_test_wait_exit";
    env.run(&["spin", group, "--", "sh", "-c", "sleep 1"]);
    env.run(&["draw", group]);

    let (_, stderr, code) = env.run(&["wait", group]);
    assert_eq!(
        code, 0,
        "wait should succeed once the process exits: {}",
        stderr
    );
}

#[test]
fn wait_stopped_group_returns_immediately() {
    let env = TestEnv::new();
    let group = "_test_wait_stopped";
    env.run(&["spin", group, "--", "sleep", "60"]);

    let (_, stderr, code) = env.run(&["wait", group]);
    assert_eq!(
        code, 0,
        "waiting on a stopped group should exit 0: {}",
        stderr
    );
}

#[test]
fn init_writes_starter_config_and_refuses_overwrite() {
    let env = TestEnv::new();
    let path = env.dir.join("starter.yaml");
    let path_str = path.to_string_lossy().to_string();

    let (_, stderr, code) = env.run(&["init", &path_str]);
    assert_eq!(code, 0, "init failed: {}", stderr);
    assert!(path.exists(), "init should create the config file");
    let content = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        content.contains("groups:"),
        "starter template should have groups"
    );

    let (_, stderr, code) = env.run(&["init", &path_str]);
    assert_ne!(code, 0, "second init should fail");
    assert!(
        stderr.contains("already exists"),
        "Expected overwrite refusal: {}",
        stderr
    );
}

// ─── env expansion ─────────────────────────────────────────────────────────

#[test]
fn config_env_block_is_applied_to_process() {
    let env = TestEnv::new();
    let cfg = env.config(
        "groups:\n  greet:\n    cmd: sh -c 'echo \"hello $WHO\"'\n    env:\n      WHO: world\n",
    );
    let cfg_str = cfg.to_string_lossy();
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "spin", "greet"]);
    assert_eq!(code, 0, "spin failed: {}", stderr);
    env.run(&["--config", &cfg_str, "draw", "greet"]);
    thread::sleep(Duration::from_millis(500));

    let log = env.log("greet");
    assert!(
        log.contains("hello world"),
        "env block / expansion should reach the process: {}",
        log
    );
}

#[test]
fn completions_are_generated_for_common_shells() {
    let env = TestEnv::new();
    for shell in ["bash", "zsh", "fish"] {
        let (stdout, stderr, code) = env.run(&["completions", shell]);
        assert_eq!(code, 0, "completions {} failed: {}", shell, stderr);
        assert!(
            stdout.contains("spin"),
            "{} completions should mention 'spin': {}",
            shell,
            stdout
        );
        assert!(
            stdout.contains("loom"),
            "{} completions should mention 'loom': {}",
            shell,
            stdout
        );
    }
}

// ─── conventional aliases ─────────────────────────────────────────────────

#[test]
fn register_alias_behaves_like_spin() {
    let env = TestEnv::new();
    let (stdout, stderr, code) = env.run(&["register", "_test_register_alias", "--", "echo", "x"]);
    assert_eq!(code, 0, "register alias failed: {}", stderr);
    assert!(
        stdout.contains("_test_register_alias"),
        "register alias should register the group: {}",
        stdout
    );
    let raw = fs::read_to_string(env.dir.join("state.json")).unwrap_or_default();
    assert!(
        raw.contains("_test_register_alias"),
        "register alias should write state: {}",
        raw
    );
}

#[test]
fn start_alias_behaves_like_draw() {
    let env = TestEnv::new();
    let group = "_test_start_alias";
    env.run(&["spin", group, "--", "sleep", "60"]);
    let (_, stderr, code) = env.run(&["start", group]);
    assert_eq!(code, 0, "start alias failed: {}", stderr);
    thread::sleep(Duration::from_millis(300));
    assert!(
        env.pid_of(group).is_some(),
        "start alias should launch the group"
    );
}

#[test]
fn stop_alias_behaves_like_cut() {
    let env = TestEnv::new();
    let group = "_test_stop_alias";
    env.run(&["spin", group, "--", "sleep", "60"]);
    env.run(&["draw", group]);
    thread::sleep(Duration::from_millis(300));
    let pid = env.pid_of(group).expect("PID after draw");

    let (_, stderr, code) = env.run(&["stop", group]);
    assert_eq!(code, 0, "stop alias failed: {}", stderr);
    assert!(
        env.pid_of(group).is_none(),
        "stop alias should clear the PID from state"
    );
    let alive = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(!alive, "stop alias should terminate the process");
}

#[test]
fn status_alias_behaves_like_loom() {
    let env = TestEnv::new();
    env.run(&["spin", "_test_status_alias", "--", "echo", "x"]);
    let (stdout, _, _) = env.run(&["status"]);
    assert!(
        stdout.contains("NAME") && stdout.contains("STATUS"),
        "status alias should print the loom dashboard: {}",
        stdout
    );
}

// ─── config validation ─────────────────────────────────────────────────────

#[test]
fn spin_rejects_config_with_missing_dependency() {
    let env = TestEnv::new();
    let cfg = env.config("groups:\n  web:\n    cmd: npm start\n    depends: [ghost]\n");
    let cfg_str = cfg.to_string_lossy();
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "spin", "web"]);
    assert_ne!(code, 0, "Expected non-zero exit for invalid config");
    assert!(
        stderr.contains("ghost") || stderr.contains("not defined"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn spin_rejects_config_with_empty_cmd() {
    let env = TestEnv::new();
    let cfg = env.config("groups:\n  broken:\n    cmd: \"   \"\n");
    let cfg_str = cfg.to_string_lossy();
    let (_, stderr, code) = env.run(&["--config", &cfg_str, "spin", "broken"]);
    assert_ne!(code, 0, "Expected non-zero exit for empty cmd");
    assert!(stderr.contains("must not be empty"), "stderr: {}", stderr);
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Creates a temporary directory that is removed when the returned guard is dropped.
fn tempdir() -> TempDir {
    let path = PathBuf::from(format!(
        "/tmp/fates-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    fs::create_dir_all(&path).expect("Failed to create temp dir");
    TempDir(path)
}

struct TempDir(PathBuf);

impl std::ops::Deref for TempDir {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
