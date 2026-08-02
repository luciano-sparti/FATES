# FATES ⎊

[![CI](https://github.com/Luciano-Sparti/FATES/actions/workflows/ci.yml/badge.svg)](https://github.com/Luciano-Sparti/FATES/actions/workflows/ci.yml)
![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-000000?logo=rust&logoColor=white)
![Platforms](https://img.shields.io/badge/Platforms-Linux%20%7C%20macOS-lightgrey)
![License](https://img.shields.io/badge/License-None-yellow)

> *Thread management, orchestrated.*  
> Spinning creates. Drawing runs. Cutting ends.

> **A note on the name.** In Greek myth, the **Moirai** — the Three Fates — hold the thread of every life:
> *Clotho* **spins** it at birth, *Lachesis* **draws** it out to its destined length, and *Atropos* **cuts** it at death.
> FATES borrows their names because process management is the same small ritual — declare intent, let the work run, end it cleanly.

![The Three Fates — spinning, drawing, and cutting the thread of every process](images/moirai.png)

## Contents

- [Why FATES?](#why-fates-for-devops)
- [Features](#features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration Reference](#configuration-reference-fatesyaml)
- [Command Reference](#command-reference)
- [DevOps Patterns](#devops-patterns)
- [State & Logs](#state--logs)
- [Demo](#demo)
- [Development](#development)
- [Shell Completions](#shell-completions)

FATES is a lightweight, declarative **process orchestrator** for the command line. It lets you define named service groups — each with their own command, working directory, and dependencies — and manage their full lifecycle from a single binary.

No `tmux` panes to juggle. No shell scripts to maintain. Just a `fates.yaml` and three words.

## Features

- 🧵 **Declarative stacks** — one `fates.yaml` declares every service and its startup order.
- ▶️ **Three verbs** — `spin` to register, `draw` to run, `cut` to stop. That's the whole vocabulary.
- 🪢 **Dependency-aware** — `draw --all` brings your stack up in the right order; `cut --all` tears it down safely.
- 📊 **Live dashboard** — `loom` shows every process's status, CPU, memory, and uptime at a glance, with an interactive `--watch` mode.
- 🗂️ **Captured logs** — every group's output is saved, so you always have an audit trail.
- 🔒 **Crash-safe state** — atomic writes and file locks mean concurrent runs and crashes never corrupt your state.
- 🧳 **Portable & ephemeral** — state lives wherever you point it (default `/tmp/fates`); nothing persisted by accident.

---

## The lifecycle & how it compares

### The three verbs, mapped to the Moirai

```
        spin ──▶  REGISTERED  ──▶  draw  ──▶  RUNNING  ──▶  cut  ──▶  STOPPED
   (Clotho,         intent)      (Lachesis,   (alive)     (Atropos,   (released)
    who spins)                    who draws)               who cuts)
```

One `fates.yaml`, three words. `draw --all` brings a stack up in dependency order; `cut --all` tears it down the reverse way — so a full dev environment is two commands.

### How FATES compares

| Capability | **FATES** | `tmux` | `systemd` | `overmind` | `foreman` |
|---|:---:|:---:|:---:|:---:|:---:|
| Declarative stack (`fates.yaml` / Procfile / units) | ✅ | ❌ | ✅ | ⚠️ Procfile | ⚠️ Procfile |
| Dependency-ordered start / stop | ✅ `depends` | ❌ | ✅ `After=` | ❌ | ❌ |
| One command: up / down | ✅ `draw`/`cut --all` | ⚠️ manual | ⚠️ `systemctl` | ✅ | ✅ |
| Live dashboard (`loom`) | ✅ | ❌ | ❌ | ⚠️ basic | ❌ |
| Per-process captured logs | ✅ | ⚠️ manual | ✅ `journald` | ✅ | ✅ |
| Crash-safe state (atomic + locks) | ✅ | ❌ | ✅ | ❌ | ❌ |
| No root required | ✅ | ✅ | ❌ | ✅ | ✅ |
| Written in | **Rust** | C | C | Ruby | Ruby |

> A simplification, but the shape holds: FATES trades `systemd`'s machine-level power for a zero-config, rootless, stack-oriented workflow you can drop into any repo — and it adds the live `loom` dashboard and `depends`-driven ordering that `tmux` / `foreman` / `overmind` leave to you.

---

## Why FATES for DevOps?

Running a local dev stack or a CI environment means managing several processes at once: a database, an API server, a frontend build watcher, maybe a reverse proxy. The usual approach involves multiple terminal windows, `&` backgrounding, and ad-hoc `kill` commands — all state that lives in your head.

FATES replaces that with:

- **A declarative config** (`fates.yaml`) that defines what runs and in what order.
- **A persistent state file** (`/tmp/fates/state.json`) so every command knows what's already running.- **Captured logs** per-service, so you always have an audit trail even for background processes.
- **A one-line dashboard** (`loom`) that gives you the status of your entire stack at a glance.

---

## Installation

```bash
git clone https://github.com/Luciano-Sparti/FATES
cd FATES
cargo build --release   # requires Rust & Cargo (https://rustup.rs)
```

Copy the binary somewhere on your `$PATH`:

```bash
cp target/release/fates ~/.local/bin/fates
```

---

## Quick Start

**1. Define your stack in `fates.yaml`:**

```yaml
groups:
  postgres:
    cmd: postgres -D ~/data/pgdata
    cwd: ~/projects/myapp

  api:
    cmd: uvicorn main:app --reload --port 8000
    cwd: ~/projects/myapp/api
    depends: [postgres]

  frontend:
    cmd: npm run dev
    cwd: ~/projects/myapp/frontend
    depends: [api]
```

**2. Register and start your services:**

```bash
fates spin postgres
fates spin api
fates spin frontend

fates draw postgres
fates draw api
fates draw frontend
```

> *spin · draw · cut* — the three Fates, in the order a life is lived: intent, motion, release.

> **Tip:** bring the whole stack up in one command with `fates draw --all` (it respects the `depends` order), and tear it all down with `fates cut --all`.

**3. Check your stack:**

```bash
fates loom
```

```
NAME            STATUS     PID      CPU%     MEMORY     LIFETIME
-------------------------------------------------------------------
postgres        RUNNING    84201    0.2%     22.4 MB    3m 12s
api             RUNNING    84350    1.4%     88.1 MB    2m 58s
frontend        RUNNING    84489    0.0%     140.2 MB   2m 47s
```

**4. Tear it all down:**

```bash
fates cut frontend
fates cut api
fates cut postgres
```

---

## Configuration Reference (`fates.yaml`)

The config file describes your **process groups**. By default, FATES looks for `fates.yaml` in the current directory. You can override this with the `--config` flag.

```yaml
groups:
  <name>:
    cmd: <shell command to run>    # Required. The command to execute.
    cwd: <working directory>       # Optional. Defaults to the directory where fates is invoked.
    depends: [<name>, ...]         # Optional. Other groups this one depends on (for weave and draw --all).
    env:                           # Optional. Environment variables for the process.
      <VAR>: <value>
```

`cmd` supports shell-style quoting (single quotes, double quotes, backslashes). `$VAR` / `${VAR}` references in `cmd` and `cwd` are expanded from the process environment, with the group's `env:` block taking precedence; unset variables expand to empty. A leading `~` in `cmd` or `cwd` is expanded to your home directory.

Get started with a starter template:

```bash
fates init
# → Wrote starter config to 'fates.yaml'.
```

### Validation Rules

FATES validates your config on every `spin` and every `draw --all`. It will refuse to register a group if:

- `cmd` is empty or whitespace-only.
- A group in `depends` is not defined elsewhere in the config.
- A group lists itself as a dependency.

---

## Command Reference

Every command has a conventional alias so you don't have to memorize the weaving metaphor:

The three core verbs are named for the Moirai: **`spin`** (Clotho, who begins), **`draw`** (Lachesis, who sustains), and **`cut`** (Atropos, who ends) — with `loom`, `weave`, and `omen` as the watchers who read the thread.

| Command | What it does | Alias(es) |
|---|---|---|
| `spin` | Register a process group (declares intent, doesn't start) | `register` |
| `draw` | Start / resume a process group in the background | `start` |
| `cut` | Gracefully (or forcefully) stop a process group | `stop` |
| `loom` | One-line dashboard of all groups | `status` |
| `weave` | ASCII dependency tree of a group | `tree` |
| `omen` | Inspect a group's lifecycle, logs, resources | `info` |
| `logs` | Print a group's captured output | `log` |
| `wait` | Block until a group's process exits | — |
| `reel` | Rotate a group's log file (copy + truncate) | `rotate` |
| `respin` | Stop a group if running, then start it fresh | `restart` |
| `init` | Write a starter `fates.yaml` template | — |
| `completions` | Generate shell completions (`bash`, `zsh`, `fish`, …) | — |

All commands support the global `--config` / `-c` flag to point to a non-default config file:

```bash
fates --config /path/to/stack.yaml <command>
```

All commands also support `--state-dir <dir>` to override where the state file and logs live (defaults to `$FATES_STATE_DIR`, then `/tmp/fates`):

```bash
fates --state-dir ~/.fates/myapp loom
```

---

### `fates spin <name> [-- <cmd> <args>...]`

**Register a process group** with FATES. This records the group's command and configuration in the state file, but does not start it yet. Think of it as declaring intent.

The command can come from one of two sources (in order of priority):

1. **Inline:** Provided after `--` on the command line.
2. **Config file:** Defined in `fates.yaml` under `groups.<name>.cmd`.

```bash
# From config file
fates spin api

# Inline command (overrides config)
fates spin redis -- redis-server --port 6380

# With a custom config
fates --config ~/stacks/prod.yaml spin api
```

> **Note:** `spin` validates the entire config file and exits with an error if any group has invalid definitions.

---

### `fates draw <name>` / `fates draw --all`

**Start a registered process group** in the background. The process is detached from your terminal; its stdout and stderr are captured to `<state-dir>/<name>.log`.

`draw` returns immediately — your terminal is free the moment the process is spawned. If the group is already running, `draw` will refuse to start a second instance.

With `--all`, every registered group is started in dependency order (as declared by `depends`), so a full stack comes up in one command:

```bash
fates draw --all
```

`draw --all` is fully **config-driven**: any group defined in `fates.yaml` that was never `spin`-ed is registered automatically (with a `Note: registered '…' from config.` line), so a fresh checkout can go straight from `fates init` to `fates draw --all`.

Quoted arguments, `$VAR` expansion, and `~` in `cmd` / `cwd` are handled correctly — commands are split like a shell would, not on raw whitespace.

```bash
fates draw postgres
# → Thread 'postgres' drawing. PID: 84201. Logs: /tmp/fates/postgres.log
```

```bash
# Check the live logs
tail -f /tmp/fates/postgres.log

# Or use fates itself
fates logs postgres --tail 50
```---

### `fates cut <name> [--force]` / `fates cut --all [--force]`

**Stop a running process group.**

With `--all`, every registered group is stopped in reverse dependency order (dependents first):

Without `--force`, FATES sends `SIGTERM` and waits up to **5 seconds** for the process to exit cleanly. This allows the process to handle graceful shutdown (flush writes, close connections, etc.). If the process is still alive after the grace period, `SIGKILL` is sent automatically.

With `--force`, `SIGKILL` is sent immediately with no grace period.

```bash
# Graceful shutdown (recommended)
fates cut api

# Immediate termination
fates cut api --force
```

The signal is sent to the **entire process group**, so any child processes spawned by the command are also terminated.

### `fates respin <name>` / `fates respin --all [--force]` *(alias: `fates restart`)*

**Stop a process group if it is running, then spin it up fresh.** Useful after deploying new code or changing environment variables:

```bash
fates respin api
fates respin api --force   # hard-kill during the stop phase
fates respin --all         # respin the whole stack in dependency order

# the conventional name also works
fates restart api
```

If the group is already stopped, `respin` just draws it. Stale PIDs (dead or reused) are cleared before the fresh start.

---

### `fates loom`

**Print a live dashboard** of all registered process groups. For each group, it shows:

| Column | Description |
|---|---|
| `NAME` | Group name as registered with `spin` |
| `STATUS` | `RUNNING` or `STOPPED` |
| `PID` | OS process ID (or `-` if stopped) |
| `CPU%` | Current CPU usage percentage |
| `MEMORY` | Resident memory usage |
| `LIFETIME` | How long the process has been running |

```bash
fates loom
```

```
NAME            STATUS     PID      CPU%     MEMORY     LIFETIME
-------------------------------------------------------------------
postgres        RUNNING    84201    0.2%     22.4 MB    3m 12s
api             RUNNING    84350    1.4%     88.1 MB    2m 58s
redis           STOPPED    -        -        -          -
```

`loom` also self-heals the state file: if a group has a recorded PID that no longer exists on the system — or whose PID has been reused by an unrelated process — it automatically marks that group as `STOPPED`. PIDs are verified against `/proc/<pid>/cmdline`, so a reused PID is never mistaken for your process.

For scripting and machine consumption, `fates loom --json` emits the same data as a JSON array instead of the table:

```bash
fates loom --json
```

```json
[
  {
    "name": "api",
    "status": "RUNNING",
    "pid": 84350,
    "cmd": "npm start",
    "cwd": "/srv/app",
    "cpu_percent": 1.4,
    "memory_bytes": 92405760,
    "uptime_seconds": 178
  },
  {
    "name": "redis",
    "status": "STOPPED",
    "pid": null,
    "cmd": "redis-server",
    "cwd": null,
    "cpu_percent": null,
    "memory_bytes": null,
    "uptime_seconds": null
  }
]
```

Fields are `name`, `status` (`"RUNNING"`/`"STOPPED"`), `pid`, `cmd`, `cwd`, `cpu_percent`, `memory_bytes`, and `uptime_seconds`; the metric fields are `null` for stopped groups. An empty state prints `[]`.

For a live dashboard, `fates loom --watch` refreshes in place every 2 seconds (or pass an interval: `--watch 5`). When stdout is a terminal, it becomes a full interactive dashboard: box-drawn table with green `RUNNING` / red `STOPPED` status, CPU% tinted by load, a `Totals` footer, and an alternate screen (your shell history is untouched on exit):

```bash
fates loom --watch
fates loom --watch 5
```

| Key | Action |
|---|---|
| `↑`/`↓` or `j`/`k` | Move the selection highlight |
| `Enter`, `Space`, or `l` | Toggle the log pane (last 15 lines of the selected group) |
| `q` or `Ctrl-C` | Quit (restores the terminal) |

Piped through a non-terminal, `loom --watch` falls back to clearing and re-printing the plain table, so it still works in scripts and CI. `loom` itself auto-detects: colors are only emitted to a terminal — `fates loom | jq` piping stays byte-clean.

In a terminal, `loom --watch` becomes a box-drawn dashboard on the alternate screen (your scrollback is untouched on exit):

```
┌─ FATES · loom ──────────────────────────────────────────────────┐
│ NAME        STATUS     PID      CPU%     MEMORY     LIFETIME     │
│ postgres    RUNNING    84201    0.2%     22.4 MB    3m 12s       │
│ api         RUNNING    84350    1.4%     88.1 MB    2m 58s       │
│ frontend    RUNNING    84489    0.0%     140.2 MB   2m 47s       │
│ redis       STOPPED    -        -        -          -            │
│ ── Totals: 3 running · 1 stopped · CPU 1.6% · 250.7 MB ──────────│
└─────────────────────────────────────────────────────────────────┘
  ↑/↓ select · Enter toggle logs · q quit
```

The same check protects the dangerous operations: `cut` refuses to signal a stale PID (a negative-PID kill would otherwise hit whatever process now owns it), and `draw` clears stale PIDs and starts a fresh instance instead of claiming the group is still running.

---

### `fates weave <name>`

**Visualise the dependency tree** of a process group as an ASCII diagram. Dependencies are read from the `depends` field in `fates.yaml`.

```bash
fates weave frontend
```

```
frontend
└── api
    └── postgres
```

Useful for understanding startup order in complex stacks. FATES will detect and report circular dependencies.

---

### `fates omen <name>`

**Inspect a single process group** in detail. Shows configuration and real-time resource usage pulled directly from the OS.

```bash
fates omen api
```

```
========================================
 OMEN: api
========================================
Command:     uvicorn main:app --reload --port 8000
CWD:         /home/user/projects/myapp/api
Status:      RUNNING
PID:         84350
CPU Usage:   1.4%
Memory:      88.1 MB (92372992 bytes)
Uptime:      0h 2m 58s (178 seconds)
Executable:  /usr/bin/python3
========================================
```

---

### `fates logs <name> [--tail <n>] [--follow]`

**Print the captured output** (stdout + stderr) of a process group. All output from `draw` is redirected to `<state-dir>/<name>.log`. The log content is printed raw (no headers), so you can pipe it straight into other tools.

```bash
# Print all logs
fates logs api

# Print the last 100 lines (reads from the end of the file, no full-file load)
fates logs api --tail 100

# Live follow, like `tail -f`
fates logs api --follow
# or the short form
fates logs api -f
```

`--tail` scans the file backwards in chunks, so it stays fast even on very large logs. `--follow` prints only newly appended bytes and transparently re-opens the file if it is rotated (e.g. by `logrotate`), continuing on the fresh file. Combine them for classic `tail -f -n N` behavior:

```bash
# Print the last 50 lines, then keep following new output
fates logs api --follow --tail 50
```

### `fates wait <name> [--timeout <secs>]`

**Block until a group's process exits**, then return exit code `0`. Useful in scripts and CI when you run a task to completion:

```bash
fates draw migrations && fates wait migrations

# Don't wait forever
fates wait api --timeout 30   # exits 1 with "Timed out" if still running
```

If the group is already stopped, `wait` returns immediately. Polling checks `/proc/<pid>/cmdline`, so a reused PID is never mistaken for the process still running.

### `fates reel <name> [--keep <n>]` *(alias: `fates rotate`)*

**Reel a group's log file**: archive the current contents to `<name>.log.1`, shift older backups down (`<name>.log.1` → `<name>.log.2`, etc.), and truncate the live file. Keeps `--keep` backups (default `5`).

```bash
fates reel api
fates reel api --keep 10

# the conventional name also works
fates rotate api
```

```
Archived /tmp/fates/api.log (20480 bytes) to /tmp/fates/api.log.1
Reeled log for 'api' (keeping 5 backup(s)).
```

Reeling is **copy + truncate** rather than rename, deliberately: running processes hold their log open with `O_APPEND`, so after truncation their next write lands at the start of the fresh file. `fates logs api --follow` detects the size drop and follows the new content seamlessly.

---

## DevOps Patterns

### Pattern 1: Full local dev stack

```yaml
# fates.yaml
groups:
  db:
    cmd: docker compose up db
    cwd: ~/projects/myapp

  migrations:
    cmd: alembic upgrade head
    cwd: ~/projects/myapp/api
    depends: [db]

  api:
    cmd: uvicorn main:app --reload
    cwd: ~/projects/myapp/api
    depends: [db]

  worker:
    cmd: celery -A app worker --loglevel=info
    cwd: ~/projects/myapp
    depends: [db]

  frontend:
    cmd: npm run dev
    cwd: ~/projects/myapp/frontend
    depends: [api]
```

```bash
# Register the whole stack
fates spin db
fates spin migrations
fates spin api
fates spin worker
fates spin frontend

# Bring it all up, dependencies first
fates draw --all

# Check everything
fates loom

# Tear down, dependents first
fates cut --all
```

### Pattern 2: Switching between environments

Keep separate config files per environment and switch with `--config`. Give each environment its own state dir so running instances never collide:

```bash
alias fates-local='fates --config stacks/local.yaml --state-dir /tmp/fates-local'
alias fates-staging='fates --config stacks/staging.yaml --state-dir /tmp/fates-staging'

fates-local spin api && fates-local draw --all
fates-staging spin api && fates-staging draw --all

fates-local loom
fates-local cut --all
```

### Pattern 3: CI pre-flight checks

Use FATES in CI scripts to bring up dependencies before running tests:

```bash
#!/bin/bash
set -e

fates spin postgres -- postgres -D /tmp/pgdata
fates draw postgres

# Wait for postgres to be ready
until pg_isready -q; do sleep 0.5; done

# Run tests
pytest tests/

# Tear down
fates cut postgres
```

### Pattern 4: Debugging a misbehaving service

```bash
# Is it running at all?
fates loom

# What are its resource numbers?
fates omen api

# What did it log?
fates logs api --tail 200

# Hard respin
fates cut api --force
fates draw api
```

---

## State & Logs

| Path | Purpose |
|---|---|
| `<state-dir>/state.json` | Runtime state: registered groups, PIDs, commands |
| `<state-dir>/state.lock` | Advisory lock serializing state updates between concurrent `fates` invocations |
| `<state-dir>/<name>.log` | Captured stdout + stderr per group |

The state directory defaults to `/tmp/fates` but can be overridden with the `--state-dir` flag or the `FATES_STATE_DIR` environment variable. This lets you keep separate stacks isolated (e.g. one state dir per project or environment).

Every command that reads or writes state takes an exclusive `flock` on `state.lock`, so concurrent invocations are serialized instead of clobbering each other's updates. State is written atomically (temp file + rename), so a crash mid-write can never leave a truncated state file.

State is intentionally stored outside the project and is not persisted across reboots. This is by design for ephemeral dev and CI environments.

---

## Demo

`./demo.sh` runs a self-contained tour of every command against a throwaway state dir: it builds the binary if needed, writes a temp config with a tiny `db → api → web` stack, starts it, and walks through `loom`, `weave`, `omen`, `logs`, `reel`, `respin`, `wait`, `cut`, and more. Everything is cleaned up on exit.

```bash
./demo.sh            # full tour
./demo.sh loom       # a single step, by command name
./demo.sh 6          # ... or by number (1..17)
```

## Development

Build and test locally:

```bash
cargo build
cargo test          # unit + integration tests (isolated state dirs, no cleanup needed)
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs all three gates — format check, clippy with warnings denied, and the test suite — on every push and pull request.

## Shell Completions

Generate completions for your shell and drop them into your rc file:

```bash
fates completions bash  # or: zsh, fish, elvish, powershell

# bash
fates completions bash > ~/.local/share/bash-completion/completions/fates

# zsh
fates completions zsh > ~/.zfunc/_fates
```

---

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Command error (group not found, already running, invalid config, bad arguments) |
| `2` | System error (I/O, state lock/creation failure, spawn failure) |

---

## Contributing

FATES is young and contributions are welcome — bug reports, docs, and especially new commands or platform support.

1. Fork and clone the repo.
2. `cargo build` to try it locally; `./demo.sh` for a self-contained tour of every command.
3. Keep `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` clean — CI enforces both.
4. Open a PR against `master`.

## License

FATES is currently **unlicensed** (all rights reserved). A permissive license (MIT or Apache-2.0) is planned so the code can be freely reused — tracking in an upcoming release. Until then, the source is here for reading and learning. If you'd like to use it sooner, open an issue and I'll prioritize publishing a license.
