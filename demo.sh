#!/usr/bin/env bash
# FATES — a quick tour of every command.
#
# Builds the binary if needed, spins up a tiny fake stack in a temp dir, and
# runs each subcommand (plus its conventional alias) so you can see what it
# does. Nothing is touched outside the temp dir, and everything is cleaned up
# on exit.
#
# Usage:
#   ./demo.sh                 # the full tour
#   ./demo.sh loom            # a single step, by command name
#   ./demo.sh 6               # ... or by number (1..17)
#
# Steps: 1 help · 2 init · 3 spin · 4 draw · 5 loom · 6 json · 7 weave ·
#        8 omen · 9 logs · 10 follow · 11 reel · 12 watch · 13 respin ·
#        14 wait · 15 cut · 16 completions · 17 done

set -euo pipefail
cd "$(dirname "$0")"

BIN="${FATES_BIN:-$PWD/target/debug/fates}"
if [[ ! -x "$BIN" ]]; then
  echo "Building fates first..."
  cargo build --quiet
fi

WORKDIR="$(mktemp -d -t fates-demo.XXXXXX)"
STATEDIR="$WORKDIR/state"
CONFIG="$WORKDIR/fates.yaml"

trap 'rm -rf "$WORKDIR"' EXIT

C_SECTION=$'\033[1;36m'
C_CMD=$'\033[1;33m'
C_DIM=$'\033[2m'
C_RESET=$'\033[0m'

section() { printf '\n%s▸ %s%s\n' "$C_SECTION" "$1" "$C_RESET"; }
note()    { printf '%s# %s%s\n' "$C_DIM" "$1" "$C_RESET"; }

run() {
  printf '\n%s$ fates %s%s\n' "$C_CMD" "$*" "$C_RESET"
  "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" "$@"
}

write_stack_config() {
  cat > "$CONFIG" <<'YAML'
groups:
  db:
    cmd: "sleep 600"
  api:
    cmd: "bash -c 'while true; do echo \"[api] $(date +%T) handled request\"; sleep 1; done'"
    depends: [db]
  web:
    cmd: "sleep 600"
    depends: [api]
YAML
}

STEP="${1:-all}"
names=(help init spin draw loom json weave omen logs follow reel watch respin wait cut completions done)
if [[ "$STEP" =~ ^[0-9]+$ ]]; then
  STEP="${names[$((STEP-1))]}"
fi

want() { [[ "$STEP" == "all" || "$STEP" == "$1" ]]; }

case "$STEP" in
  all|help|init) ;;
  *) write_stack_config ;;
esac

# ── 1. help ─────────────────────────────────────────────────────────────────
if want help; then
  section "1/17 — fates --help (every command + conventional alias)"
  run --help
fi

# ── 2. init ─────────────────────────────────────────────────────────────────
if want init; then
  section "2/17 — fates init (write a starter config)"
  run init "$CONFIG"
fi

# ── 3. the demo stack ───────────────────────────────────────────────────────
if [[ "$STEP" == "all" ]]; then
  section "3/17 — a small stack: db, api (depends on db), web (depends on api)"
  write_stack_config
  echo "wrote $CONFIG (replaced the starter template)."
fi

# ── 4. spin ─────────────────────────────────────────────────────────────────
if want spin; then
  section "4/17 — fates spin web (register a group from the config)"
  run spin web
fi

# ── 5. draw ─────────────────────────────────────────────────────────────────
if want draw; then
  section "5/17 — fates draw --all (start the stack; registers db + api from config)"
  run draw --all
fi

# ── 6. loom ─────────────────────────────────────────────────────────────────
if want loom; then
  section "6/17 — fates loom / status (dashboard + Totals footer)"
  run loom
fi

# ── 7. loom --json ──────────────────────────────────────────────────────────
if want json; then
  section "7/17 — fates loom --json (machine-readable)"
  run loom --json
fi

# ── 8. weave ────────────────────────────────────────────────────────────────
if want weave; then
  section "8/17 — fates weave web / tree (dependency tree)"
  run weave web
fi

# ── 9. omen ─────────────────────────────────────────────────────────────────
if want omen; then
  section "9/17 — fates omen api / info (lifecycle + resources + recent logs)"
  run omen api
fi

# ── 10. logs ────────────────────────────────────────────────────────────────
if want logs; then
  section "10/17 — fates logs api --tail 3 / log (last 3 captured lines)"
  run logs api --tail 3
fi

# ── 11. logs --follow ───────────────────────────────────────────────────────
if want follow; then
  section "11/17 — fates logs api --tail 2 --follow (live tail, killed after 3s)"
  printf '\n%s$ timeout 3 fates logs api --tail 2 --follow%s\n' "$C_CMD" "$C_RESET"
  timeout 3 "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" logs api --tail 2 --follow || true
fi

# ── 12. reel ────────────────────────────────────────────────────────────────
if want reel; then
  section "12/17 — fates reel api / rotate (archive + truncate the log)"
  run reel api --keep 3
fi

# ── 13. loom --watch ────────────────────────────────────────────────────────
if want watch; then
  section "13/17 — fates loom --watch 1 (live refresh; interactive in a real terminal)"
  printf '\n%s$ timeout 3 fates loom --watch 1%s\n' "$C_CMD" "$C_RESET"
  timeout 3 "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" loom --watch 1 || true
fi

# ── 14. respin ──────────────────────────────────────────────────────────────
if want respin; then
  section "14/17 — fates respin web / restart (stop, then start fresh)"
  run respin web
fi

# ── 15. wait ────────────────────────────────────────────────────────────────
if want wait; then
  section "15/17 — fates wait (block until a process exits)"
  printf '\n%s$ fates spin task -- sleep 3%s\n' "$C_CMD" "$C_RESET"
  "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" spin task -- sleep 3
  printf '\n%s$ fates draw task%s\n' "$C_CMD" "$C_RESET"
  "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" draw task
  printf '\n%s$ fates wait task   (blocks ~3s until the sleep exits)%s\n' "$C_CMD" "$C_RESET"
  "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" wait task
  printf '\n%s$ fates loom   (task is now STOPPED)%s\n' "$C_CMD" "$C_RESET"
  "$BIN" --config "$CONFIG" --state-dir "$STATEDIR" loom
fi

# ── 16. cut ─────────────────────────────────────────────────────────────────
if want cut; then
  section "16/17 — fates cut --all / stop (tear the whole stack down)"
  run cut --all
  run loom
fi

# ── 17. completions ─────────────────────────────────────────────────────────
if want completions; then
  section "17/17 — fates completions bash (shell completions, first lines)"
  printf '\n%s$ fates completions bash | head -6%s\n' "$C_CMD" "$C_RESET"
  "$BIN" completions bash | head -6
fi

if want done; then
  note "state + logs lived in $WORKDIR (now cleaned up)"
  echo
  echo "That's the whole tour. For details: $BIN <command> --help"
fi
