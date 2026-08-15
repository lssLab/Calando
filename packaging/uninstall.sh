#!/bin/sh
# Remove installed services, hooks, skills, and commands. Preserve state and user configuration.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
POINTER_DIR="$HOME/.memory-supervisor"
RUNTIME_DIR=${MEMORY_SUPERVISOR_RUNTIME_DIR:-"$HOME/.local/lib/memory-supervisor"}
OWNED_BINARY="$RUNTIME_DIR/memory-supervisor"
BINARY=${MEMORY_SUPERVISOR_BINARY:-}
if [ -z "$BINARY" ] && [ -r "$POINTER_DIR/binary" ]; then
  IFS= read -r BINARY < "$POINTER_DIR/binary" 2>/dev/null || true
fi
[ -n "$BINARY" ] || BINARY="$OWNED_BINARY"
if [ -n "${MEMORY_SUPERVISOR_DIR:-}" ]; then
  STATE_DIR=$MEMORY_SUPERVISOR_DIR
elif [ -r "$POINTER_DIR/state-dir" ]; then
  STATE_DIR=""
  IFS= read -r STATE_DIR < "$POINTER_DIR/state-dir" 2>/dev/null || true
  [ -n "$STATE_DIR" ] || STATE_DIR="$HOME/.cache/memory-supervisor"
else
  STATE_DIR="$HOME/.cache/memory-supervisor"
fi

# Parse and remove owned hooks before changing the running service. A malformed
# provider file must leave the working installation intact for manual repair.
if [ -x "$BINARY" ]; then
  [ ! -f "$HOME/.claude/settings.json" ] || "$BINARY" integration hooks \
    --target "$HOME/.claude/settings.json" --provider claude --binary "$BINARY" --remove
  CODEX_CONFIG_HOME=${CODEX_HOME:-"$HOME/.codex"}
  CODEX_HOOK_TARGET="$CODEX_CONFIG_HOME/hooks.json"
  DEFAULT_CODEX_HOOK_TARGET="$HOME/.codex/hooks.json"
  [ ! -f "$CODEX_HOOK_TARGET" ] || "$BINARY" integration hooks \
    --target "$CODEX_HOOK_TARGET" --provider codex --binary "$BINARY" --remove
  if [ "$DEFAULT_CODEX_HOOK_TARGET" != "$CODEX_HOOK_TARGET" ]; then
    [ ! -f "$DEFAULT_CODEX_HOOK_TARGET" ] || "$BINARY" integration hooks \
      --target "$DEFAULT_CODEX_HOOK_TARGET" --provider codex --binary "$BINARY" --remove
  fi
else
  echo "installed binary is missing; provider hook files were left unchanged" >&2
fi

if [ "$(uname -s)" = Darwin ]; then
  PLIST="$HOME/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist"
  if [ ! -L "$PLIST" ] && [ -f "$PLIST" ] && \
    grep -Fq "io.github.lsslab.memory-supervisor" "$PLIST"; then
    launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
    rm -f "$PLIST"
  fi
elif command -v systemctl >/dev/null 2>&1 && \
  systemctl --user show-environment >/dev/null 2>&1; then
  UNIT="$HOME/.config/systemd/user/memory-supervisor.service"
  if [ ! -L "$UNIT" ] && [ -f "$UNIT" ] && \
    { grep -Fq "Claude Code & Codex CLI Memory Supervisor" "$UNIT" || \
      grep -Fq "$DIR/supervisor.py" "$UNIT"; }; then
    systemctl --user disable --now memory-supervisor.service 2>/dev/null || true
    rm -f "$UNIT"
    systemctl --user daemon-reload 2>/dev/null || true
  fi
fi

if [ -r "$STATE_DIR/daemon.pid" ]; then
  pid=""
  IFS= read -r pid < "$STATE_DIR/daemon.pid" 2>/dev/null || true
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    args=$(ps -p "$pid" -o args= 2>/dev/null || true)
    case "$args" in
      *"memory-supervisor"*" daemon"*|*"$DIR/supervisor.py"*)
        kill "$pid" 2>/dev/null || true
        count=0
        while [ "$count" -lt 100 ]; do
          current_args=$(ps -p "$pid" -o args= 2>/dev/null || true)
          case "$current_args" in
            *"memory-supervisor"*" daemon"*|*"$DIR/supervisor.py"*) ;;
            *) break ;;
          esac
          sleep 0.1
          count=$((count + 1))
        done
        current_args=$(ps -p "$pid" -o args= 2>/dev/null || true)
        case "$current_args" in
          *"memory-supervisor"*" daemon"*|*"$DIR/supervisor.py"*)
            echo "owned supervisor did not stop; uninstall aborted before removing its runtime" >&2
            exit 1
            ;;
        esac
        ;;
      *) echo "preserving reused/non-supervisor pid $pid" >&2 ;;
    esac
  fi
  rm -f "$STATE_DIR/daemon.pid"
fi
if [ -f "$STATE_DIR/linger-enabled-by-install" ] && command -v loginctl >/dev/null 2>&1; then
  loginctl disable-linger "$(id -un)" 2>/dev/null || true
  rm -f "$STATE_DIR/linger-enabled-by-install"
fi

remove_owned_link() {
  installed=$1
  expected=$2
  if [ -L "$installed" ] && [ "$(readlink "$installed")" = "$expected" ]; then
    rm -f "$installed"
  fi
}
for skill in "$HOME/.claude/skills/memory-supervisor" \
  "$HOME/.agents/skills/memory-supervisor" "$HOME/.codex/skills/memory-supervisor" \
  "$HOME/.claude/skills/memory-governor" "$HOME/.agents/skills/memory-governor" \
  "$HOME/.codex/skills/memory-governor"; do
  remove_owned_link "$skill" "$DIR"
done
for command in memory-supervisor memory-status memory-control; do
  remove_owned_link "$HOME/.local/bin/$command" "$BINARY"
done

remove_unchanged_command() {
  installed=$1
  source=$2
  if [ ! -L "$installed" ] && [ -f "$installed" ] && cmp -s "$installed" "$source"; then
    rm -f "$installed"
  fi
}
remove_unchanged_command "$HOME/.claude/commands/memory-status.md" \
  "$DIR/integrations/claude/memory-status.md"
remove_unchanged_command "$HOME/.codex/prompts/memory-status.md" \
  "$DIR/integrations/codex/memory-status.md"

if [ "$BINARY" = "$OWNED_BINARY" ]; then
  rm -f "$BINARY" "$BINARY.previous"
  rmdir "$(dirname "$BINARY")" 2>/dev/null || true
fi
rm -f "$POINTER_DIR/binary" "$POINTER_DIR/install-root"
echo "removed; state, incidents, power choice, hard-cap settings, and notifications.conf were preserved"
