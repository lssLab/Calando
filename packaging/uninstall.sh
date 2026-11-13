#!/bin/sh
# Remove installed services, hooks, skills, and commands. Preserve state and user configuration.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
POINTER_DIR="$HOME/.calando"
RUNTIME_DIR=${MEMORY_SUPERVISOR_RUNTIME_DIR:-"$HOME/.local/lib/calando"}
OWNED_BINARY="$RUNTIME_DIR/calando"
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
  [ -n "$STATE_DIR" ] || STATE_DIR="$HOME/.cache/calando"
else
  STATE_DIR="$HOME/.cache/calando"
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
  PLIST="$HOME/Library/LaunchAgents/io.github.lsslab.calando.plist"
  if [ ! -L "$PLIST" ] && [ -f "$PLIST" ] && \
    grep -Fq "io.github.lsslab.calando" "$PLIST"; then
    launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
    rm -f "$PLIST"
  fi
elif command -v systemctl >/dev/null 2>&1 && \
  systemctl --user show-environment >/dev/null 2>&1; then
  UNIT="$HOME/.config/systemd/user/calando.service"
  if [ ! -L "$UNIT" ] && [ -f "$UNIT" ] && \
    { grep -Fq "Calando memory protection" "$UNIT" || \
      grep -Fq "$DIR/supervisor.py" "$UNIT"; }; then
    systemctl --user disable --now calando.service 2>/dev/null || true
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
      *"calando"*" daemon"*|*"memory-supervisor"*" daemon"*|*"$DIR/supervisor.py"*)
        kill "$pid" 2>/dev/null || true
        count=0
        while [ "$count" -lt 100 ]; do
          current_args=$(ps -p "$pid" -o args= 2>/dev/null || true)
          case "$current_args" in
            *"calando"*" daemon"*|*"memory-supervisor"*" daemon"*|*"$DIR/supervisor.py"*) ;;
            *) break ;;
          esac
          sleep 0.1
          count=$((count + 1))
        done
        current_args=$(ps -p "$pid" -o args= 2>/dev/null || true)
        case "$current_args" in
          *"calando"*" daemon"*|*"memory-supervisor"*" daemon"*|*"$DIR/supervisor.py"*)
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
is_supervisor_skill() {
  [ -r "$1/SKILL.md" ] && grep -Eq '^name: (memory-governor|memory-supervisor|calando)$' "$1/SKILL.md"
}
for root in "$HOME/.claude/skills" "$HOME/.agents/skills" "$HOME/.codex/skills"; do
  for skill in calando memory-supervisor memory-governor; do
    if is_supervisor_skill "$root/$skill"; then rm -rf "$root/$skill"; fi
  done
done
for command in calando memory-supervisor memory-status memory-control; do
  remove_owned_link "$HOME/.local/bin/$command" "$BINARY"
done
rm -f "$HOME/.local/lib/memory-supervisor/memory-supervisor"
rmdir "$HOME/.local/lib/memory-supervisor" 2>/dev/null || true

if [ "$BINARY" = "$OWNED_BINARY" ]; then
  rm -f "$BINARY" "$BINARY.previous"
  rmdir "$(dirname "$BINARY")" 2>/dev/null || true
fi
rm -f "$POINTER_DIR/binary" "$POINTER_DIR/install-root"
echo "removed; state, incidents, power choice, hard-cap settings, and notifications.conf were preserved"
