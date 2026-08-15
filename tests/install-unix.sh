#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BINARY=${MEMORY_SUPERVISOR_BINARY:-"$ROOT/target/debug/memory-supervisor"}
[ -x "$BINARY" ] || {
  echo "build the Rust binary before running installer tests: $BINARY" >&2
  exit 1
}
unset MEMORY_SUPERVISOR_BINARY

write_script() {
  path=$1
  shift
  printf '%s\n' "$@" > "$path"
  chmod 755 "$path"
}

stop_pid_file() {
  state=$1
  [ -r "$state/daemon.pid" ] || return 0
  pid=""
  IFS= read -r pid < "$state/daemon.pid" 2>/dev/null || true
  [ -n "$pid" ] || return 0
  kill "$pid" 2>/dev/null || true
}

clean_install() (
  sandbox=$(mktemp -d "${TMPDIR:-/tmp}/memory-supervisor-clean.XXXXXX")
  home="$sandbox/home"
  codex_home="$sandbox/codex-home"
  state="$sandbox/state"
  federation="$sandbox/federation"
  fake_bin="$sandbox/bin"
  nvm_bin="$home/.nvm/versions/node/v24.16.0/bin"
  mkdir -p "$home" "$fake_bin"
  trap 'stop_pid_file "$state"; rm -rf "$sandbox"' 0 HUP INT TERM

  write_script "$fake_bin/systemctl" '#!/bin/sh' 'exit 1'
  write_script "$fake_bin/claude" '#!/bin/sh' \
    '[ "${1:-}" != --version ] || echo "2.1.142 (Claude Code)"'
  mkdir -p "$nvm_bin"
  write_script "$nvm_bin/claude" '#!/bin/sh' \
    '[ "${1:-}" != --version ] || echo "2.1.220 (Claude Code)"'
  write_script "$fake_bin/codex" '#!/bin/sh' \
    'if [ "${1:-}" = --version ]; then echo "codex-cli 0.145.0"; elif [ "${1:-}" = features ]; then echo "hooks stable true"; fi'
  foreign="$sandbox/foreign"
  write_script "$foreign" '#!/bin/sh' 'exit 0'
  mkdir -p "$home/.local/bin" "$home/.agents/skills" "$home/.codex/prompts" "$codex_home"
  ln -s "$foreign" "$home/.local/bin/memory-control"
  foreign_skill="$sandbox/foreign-skill"
  mkdir "$foreign_skill"
  ln -s "$foreign_skill" "$home/.agents/skills/memory-supervisor"
  dangling="$sandbox/missing-prompt"
  ln -s "$dangling" "$home/.codex/prompts/memory-status.md"

  export HOME=$home
  export CODEX_HOME=$codex_home
  export PATH="$fake_bin:/usr/bin:/bin"
  export MEMORY_SUPERVISOR_BINARY_SOURCE=$BINARY
  export MEMORY_SUPERVISOR_SERVICE_MODE=fallback
  export MEMORY_SUPERVISOR_DIR=$state
  export MEMORY_SUPERVISOR_FEDERATION_DIR=$federation
  export MEMORY_SUPERVISOR_TICK_S=0.25
  export MEMORY_SUPERVISOR_LEAK_ACTION=none

  "$ROOT/install.sh"
  first_pid=$(sed -n '1p' "$state/daemon.pid")
  kill -0 "$first_pid"
  [ ! -e "$home/.codex/hooks.json" ]
  printf '%s\n' '{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"memory-supervisor gate codex SessionStart","timeout":5}]}]}}' \
    > "$home/.codex/hooks.json"
  "$ROOT/install.sh"
  second_pid=$(sed -n '1p' "$state/daemon.pid")
  [ "$first_pid" != "$second_pid" ]
  kill -0 "$second_pid"

  installed="$home/.local/lib/memory-supervisor/memory-supervisor"
  "$installed" integration hooks --target "$home/.claude/settings.json" \
    --provider claude --binary "$installed" --check
  "$installed" integration hooks --target "$codex_home/hooks.json" \
    --provider codex --binary "$installed" --check
  grep -Fq -- "--hook-source '$home/.codex/hooks.json'" "$home/.codex/hooks.json"
  [ "$(readlink "$home/.local/bin/memory-supervisor")" = "$installed" ]
  [ "$(readlink "$home/.local/bin/memory-status")" = "$installed" ]
  [ "$(readlink "$home/.local/bin/memory-control")" = "$foreign" ]
  [ "$(readlink "$home/.agents/skills/memory-supervisor")" = "$foreign_skill" ]
  [ -L "$home/.codex/prompts/memory-status.md" ]
  [ ! -e "$dangling" ]
  [ "$(sed -n '1p' "$home/.memory-supervisor/state-dir")" = "$state" ]
  [ "$(sed -n '1p' "$home/.memory-supervisor/federation-dir")" = "$federation" ]

  cp "$home/.claude/settings.json" "$sandbox/claude-settings.before-unsupported-probe"
  mv "$nvm_bin/claude" "$nvm_bin/claude.unavailable"
  preserve_output=$("$ROOT/install.sh" 2>&1)
  printf '%s\n' "$preserve_output" | grep -Fq \
    'any existing Memory Supervisor hook was preserved'
  cmp "$sandbox/claude-settings.before-unsupported-probe" "$home/.claude/settings.json"
  "$installed" status --connections | grep -Fq 'hook: connected and preserved'
  mv "$nvm_bin/claude.unavailable" "$nvm_bin/claude"
  "$ROOT/install.sh" >/dev/null
  "$installed" integration hooks --target "$home/.claude/settings.json" \
    --provider claude --binary "$installed" --check

  "$installed" off
  [ -f "$home/.memory-supervisor/power-off" ]
  ! kill -0 "$second_pid" 2>/dev/null
  "$installed" status --json | tr -d '[:space:]' | grep -q '"power":"off"'
  "$installed" status --connections | grep -q 'Core daemon.*OFF'
  hook_output=$(printf '{}' | "$installed" gate claude SessionStart)
  [ -z "$hook_output" ]

  "$ROOT/install.sh"
  [ -f "$home/.memory-supervisor/power-off" ]
  [ ! -e "$state/daemon.pid" ]
  "$installed" status --connections | grep -q 'Core daemon.*OFF'

  "$installed" on
  [ ! -e "$home/.memory-supervisor/power-off" ]
  powered_pid=$(sed -n '1p' "$state/daemon.pid")
  kill -0 "$powered_pid"
  "$installed" status --json >/dev/null

  printf '%s\n' 'user customization' > "$home/.claude/commands/memory-status.md"
  "$ROOT/uninstall.sh"
  [ ! -e "$home/.local/bin/memory-supervisor" ]
  [ ! -e "$home/.local/bin/memory-status" ]
  [ "$(sed -n '1p' "$home/.claude/commands/memory-status.md")" = 'user customization' ]
  [ "$(readlink "$home/.local/bin/memory-control")" = "$foreign" ]
  [ "$(readlink "$home/.agents/skills/memory-supervisor")" = "$foreign_skill" ]
  [ -L "$home/.codex/prompts/memory-status.md" ]

  rm -f "$home/.local/bin/memory-control"
  ln -s "$installed" "$home/.local/bin/memory-control"
  "$ROOT/install.sh"
  [ ! -e "$home/.local/bin/memory-control" ]
  [ "$(readlink "$home/.local/bin/memory-supervisor")" = "$installed" ]
)

create_legacy_runtime_fixture() {
  destination=$1
  mkdir -p "$destination/notify"
  write_script "$destination/supervisor.py" \
    '#!/usr/bin/env python3' \
    'import json, os, time' \
    'state_dir = os.environ["MEMORY_SUPERVISOR_DIR"]' \
    'os.makedirs(state_dir, exist_ok=True)' \
    'state_file = os.path.join(state_dir, "state.json")' \
    'while True:' \
    '    payload = {"schema_version": 5, "updated_at": time.time(), "level": "GREEN", "sensor_ok": True}' \
    '    temporary = state_file + ".tmp"' \
    '    with open(temporary, "w", encoding="utf-8") as handle:' \
    '        json.dump(payload, handle)' \
    '    os.replace(temporary, state_file)' \
    '    time.sleep(0.25)'
  for file in memory_supervisor_config.py memory_supervisor_events.py \
    memory_supervisor_platform.py notify/notify.py notify/terminal_notice.py; do
    printf '%s\n' '# compatibility fixture' > "$destination/$file"
  done
}

start_python_runtime() {
  root=$1
  state=$2
  federation=$3
  MEMORY_SUPERVISOR_DIR="$state" \
  MEMORY_SUPERVISOR_FEDERATION_DIR="$federation" \
  MEMORY_SUPERVISOR_TICK_S=0.25 \
  MEMORY_SUPERVISOR_LEAK_ACTION=none \
    nohup python3 "$root/supervisor.py" \
      >"$state/daemon.out.log" 2>"$state/daemon.err.log" &
  printf '%s\n' "$!" > "$state/daemon.pid"
}

wait_for_state() {
  state=$1
  count=0
  while [ ! -s "$state/state.json" ]; do
    count=$((count + 1))
    [ "$count" -lt 100 ] || {
      echo "supervisor state was not published" >&2
      return 1
    }
    sleep 0.1
  done
}

legacy_upgrade_preserves_paused_identity() (
  command -v python3 >/dev/null 2>&1 || {
    echo "python3 is required only to simulate a legacy runtime upgrade" >&2
    exit 1
  }
  [ "$(uname -s)" = Linux ] || {
    echo "paused Python-runtime cutover canary is Linux-only" >&2
    exit 1
  }
  sandbox=$(mktemp -d "${TMPDIR:-/tmp}/memory-supervisor-upgrade.XXXXXX")
  home="$sandbox/home"
  state="$sandbox/state"
  federation="$sandbox/federation"
  old_root="$sandbox/python-runtime"
  mkdir -p "$home/.memory-supervisor" "$state" "$federation" "$old_root"
  canary_pid=""
  trap 'stop_pid_file "$state"; [ -z "$canary_pid" ] || { kill -CONT "$canary_pid" 2>/dev/null || true; kill "$canary_pid" 2>/dev/null || true; }; rm -rf "$sandbox"' 0 HUP INT TERM
  create_legacy_runtime_fixture "$old_root"
  printf '%s\n' "$old_root" > "$home/.memory-supervisor/install-root"
  printf '%s\n' "$state" > "$home/.memory-supervisor/state-dir"
  printf '%s\n' "$federation" > "$home/.memory-supervisor/federation-dir"

  /bin/sleep 120 &
  canary_pid=$!
  start_token=$(awk '{print $22}' "/proc/$canary_pid/stat")
  identity="$canary_pid:$start_token"
  kill -STOP "$canary_pid"
  now=$(date +%s)
  printf '%s\n' "{\"schema_version\":2,\"updated_at\":$now,\"instance\":\"python-upgrade\",\"stopped\":{\"$canary_pid\":\"$identity\"},\"resume_cooldown\":{},\"incidents\":[{\"id\":\"upgrade-canary\",\"status\":\"suspended\",\"pid\":$canary_pid,\"identity\":\"$identity\",\"name\":\"sleep\",\"role\":\"worker\",\"reason\":\"runaway-memory\",\"recovery_policy\":\"lead-or-owner\",\"updated_at\":$now}],\"notification_events\":[],\"level\":\"GREEN\",\"level_since\":0,\"last_assessment_action\":\"allow\",\"action_since\":0,\"pending_control\":null,\"probation\":null,\"last_pressure_action_at\":0}" > "$state/runtime.json"
  start_python_runtime "$old_root" "$state" "$federation"
  wait_for_state "$state"
  case "$(ps -o stat= -p "$canary_pid")" in T*) ;; *) echo "canary was not paused" >&2; exit 1 ;; esac

  HOME=$home \
  CODEX_HOME=$home/.codex \
  PATH="/usr/bin:/bin" \
  MEMORY_SUPERVISOR_BINARY_SOURCE=$BINARY \
  MEMORY_SUPERVISOR_SERVICE_MODE=fallback \
  MEMORY_SUPERVISOR_DIR=$state \
  MEMORY_SUPERVISOR_FEDERATION_DIR=$federation \
  MEMORY_SUPERVISOR_TICK_S=0.25 \
  MEMORY_SUPERVISOR_LEAK_ACTION=none \
    "$ROOT/install.sh"
  installed="$home/.local/lib/memory-supervisor/memory-supervisor"
  HOME=$home CODEX_HOME=$home/.codex MEMORY_SUPERVISOR_DIR=$state MEMORY_SUPERVISOR_FEDERATION_DIR=$federation \
    "$installed" status --json | tr -d '[:space:]' | grep -q "\"stopped_pids\":\[$canary_pid\]"
  HOME=$home CODEX_HOME=$home/.codex MEMORY_SUPERVISOR_DIR=$state MEMORY_SUPERVISOR_FEDERATION_DIR=$federation \
    "$installed" resume "$canary_pid" --timeout 3
  case "$(ps -o stat= -p "$canary_pid")" in T*) echo "canary remained paused" >&2; exit 1 ;; esac
  HOME=$home CODEX_HOME=$home/.codex MEMORY_SUPERVISOR_DIR=$state "$ROOT/uninstall.sh"
)

failed_activation_restores_legacy_runtime() (
  command -v python3 >/dev/null 2>&1 || {
    echo "python3 is required only to simulate a legacy runtime rollback" >&2
    exit 1
  }
  sandbox=$(mktemp -d "${TMPDIR:-/tmp}/memory-supervisor-rollback.XXXXXX")
  home="$sandbox/home"
  state="$sandbox/state"
  federation="$sandbox/federation"
  old_root="$sandbox/python-runtime"
  fake="$sandbox/fake-runtime"
  mkdir -p "$home/.memory-supervisor" "$state" "$federation" "$old_root"
  trap 'stop_pid_file "$state"; rm -rf "$sandbox"' 0 HUP INT TERM
  create_legacy_runtime_fixture "$old_root"
  printf '%s\n' "$old_root" > "$home/.memory-supervisor/install-root"
  printf '%s\n' "$state" > "$home/.memory-supervisor/state-dir"
  printf '%s\n' "$federation" > "$home/.memory-supervisor/federation-dir"
  start_python_runtime "$old_root" "$state" "$federation"
  wait_for_state "$state"
  old_pid=$(sed -n '1p' "$state/daemon.pid")

  write_script "$fake" '#!/bin/sh' \
    'case "${1:-}:${2:-}:${3:-}" in' \
    '  --version::) echo "memory-supervisor test-failure"; exit 0 ;;' \
    '  integration:migrate-names:) exit 0 ;;' \
    '  integration:path:state) printf "%s\n" "$MEMORY_SUPERVISOR_DIR"; exit 0 ;;' \
    '  integration:path:federation) printf "%s\n" "$MEMORY_SUPERVISOR_FEDERATION_DIR"; exit 0 ;;' \
    '  daemon:--foreground:) exit 1 ;;' \
    '  status:--json:) exit 1 ;;' \
    'esac' \
    'exit 0'
  if HOME=$home \
    CODEX_HOME=$home/.codex \
    PATH="/usr/bin:/bin" \
    MEMORY_SUPERVISOR_BINARY_SOURCE=$fake \
    MEMORY_SUPERVISOR_SERVICE_MODE=fallback \
    MEMORY_SUPERVISOR_DIR=$state \
    MEMORY_SUPERVISOR_FEDERATION_DIR=$federation \
      "$ROOT/install.sh"; then
    echo "broken candidate unexpectedly activated" >&2
    exit 1
  fi
  new_pid=$(sed -n '1p' "$state/daemon.pid")
  [ "$new_pid" != "$old_pid" ]
  kill -0 "$new_pid"
  ps -p "$new_pid" -o args= | grep -Fq "$old_root/supervisor.py"
  [ -f "$old_root/.memory-supervisor-python-rollback" ]
  [ "$(sed -n '1p' "$home/.memory-supervisor/install-root")" = "$old_root" ]
  [ ! -e "$home/.local/lib/memory-supervisor/memory-supervisor" ]
)

clean_install
echo "PASS: clean install, reinstall, ownership preservation, and uninstall"
case "${1:-}" in
  --upgrade)
    legacy_upgrade_preserves_paused_identity
    echo "PASS: legacy-to-Rust upgrade preserves and resumes the exact paused PID"
    ;;
  --rollback)
    failed_activation_restores_legacy_runtime
    echo "PASS: failed Rust activation restores the legacy runtime"
    ;;
  --all)
    legacy_upgrade_preserves_paused_identity
    echo "PASS: legacy-to-Rust upgrade preserves and resumes the exact paused PID"
    failed_activation_restores_legacy_runtime
    echo "PASS: failed Rust activation restores the legacy runtime"
    ;;
  '') ;;
  *) echo "usage: $0 [--upgrade|--rollback|--all]" >&2; exit 2 ;;
esac
