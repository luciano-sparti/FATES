# FATES ⎊ — Plan of Action

Target: build a working CLI in 3 milestones.

## Milestone 1 — Core Three Commands
Goal: a compiled binary that can `spin`, `draw`, and `cut`.

1. `cargo init` in this folder
2. Add dependencies to `Cargo.toml`:
   - `clap` with derive features (arg parsing)
   - `nix` or `sysinfo` (process spawning / signals)
   - `serde` + `serde_yaml` (config)
3. Implement `spin` — spawn process group, write state file
4. Implement `draw` — resume by name, attach stdin/out
5. Implement `cut` — send SIGTERM then SIGKILL with timeout
6. Add `fates.yaml` loader and group registry

Acceptance: binary runs, can manage one foreground process.

## Milestone 2 — Dashboard
Goal: `fates loom` and `fates weave`.

7. Implement process state tracker (pid, status, cpu%, lifetime)
8. Build `loom` output formatter (compact table)
9. Build `weave` ASCII tree view
10. Add `fates omen <name>` — inspect runtime metadata

Acceptance: `loom` prints accurate table in one terminal line width.

## Milestone 3 — Polish
Goal: usable developer tool.

11. `--config path` flag and config validation
12. Signal handling for Ctrl-C graceful shutdown
13. `--force` flag behavior and log capture
14. docs + examples
15. packaging: `cargo build --release`, optional install script

Acceptance: clean `fates.yaml` driven workflow, ready for daily use.

## Notes
- Start with foreground process (no `screen`/`tmux` complexity).
- State temporarily in `/tmp/fates` or local SQLite later.
- `loom` must fit in one terminal line width first.
