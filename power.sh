#!/bin/sh
# Persistent on/off control for Linux, WSL, and macOS. Windows uses power.ps1.
set -eu

ACTION=${1:-}
case "$ACTION" in
  on|off) ;;
  *) echo "usage: memory-supervisor on|off" >&2; exit 2 ;;
esac

POINTER_DIR="$HOME/.memory-supervisor"
MARKER="$POINTER_DIR/power-off"
BINARY=${MEMORY_SUPERVISOR_BINARY:-}
if [ -z "$BINARY" ] && [ -r "$POINTER_DIR/binary" ]; then
  IFS= read -r BINARY < "$POINTER_DIR/binary" 2>/dev/null || true
fi
[ -x "$BINARY" ] || { echo "installed Memory Supervisor binary is missing; run memory-supervisor update" >&2; exit 1; }

if [ -n "${MEMORY_SUPERVISOR_DIR:-}" ]; then
  STATE_DIR=$MEMORY_SUPERVISOR_DIR
elif [ -r "$POINTER_DIR/state-dir" ]; then
  STATE_DIR=""
  IFS= read -r STATE_DIR < "$POINTER_DIR/state-dir" 2>/dev/null || true
  [ -n "$STATE_DIR" ] || STATE_DIR="$HOME/.cache/memory-supervisor"
else
  STATE_DIR="$HOME/.cache/memory-supervisor"
fi
if [ -n "${MEMORY_SUPERVISOR_FEDERATION_DIR:-}" ]; then
  FEDERATION_DIR=$MEMORY_SUPERVISOR_FEDERATION_DIR
elif [ -r "$POINTER_DIR/federation-dir" ]; then
  FEDERATION_DIR=""
  IFS= read -r FEDERATION_DIR < "$POINTER_DIR/federation-dir" 2>/dev/null || true
  [ -n "$FEDERATION_DIR" ] || FEDERATION_DIR="$POINTER_DIR/instances"
else
  FEDERATION_DIR="$POINTER_DIR/instances"
fi

case "${MEMORY_SUPERVISOR_SERVICE_MODE:-auto}" in
  fallback) SERVICE_KIND=fallback ;;
  auto)
    if [ "$(uname -s)" = Darwin ]; then
      SERVICE_KIND=launchd
    elif command -v systemctl >/dev/null 2>&1 && \
      systemctl --user show-environment >/dev/null 2>&1; then
      SERVICE_KIND=systemd
    else
      SERVICE_KIND=fallback
    fi
    ;;
  *) echo "MEMORY_SUPERVISOR_SERVICE_MODE must be auto or fallback" >&2; exit 1 ;;
esac

UNIT="$HOME/.config/systemd/user/memory-supervisor.service"
PLIST="$HOME/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist"
LABEL="io.github.lsslab.memory-supervisor"

case "$SERVICE_KIND" in
  systemd)
    [ ! -L "$UNIT" ] && [ -f "$UNIT" ] && grep -Fq "Claude Code & Codex CLI Memory Supervisor" "$UNIT" || {
      echo "owned Memory Supervisor service is missing; run memory-supervisor update" >&2
      exit 1
    }
    ;;
  launchd)
    [ ! -L "$PLIST" ] && [ -f "$PLIST" ] && grep -Fq "$LABEL" "$PLIST" || {
      echo "owned Memory Supervisor launch agent is missing; run memory-supervisor update" >&2
      exit 1
    }
    ;;
esac

write_marker() {
  mkdir -p "$POINTER_DIR"
  chmod 700 "$POINTER_DIR" 2>/dev/null || true
  umask 077
  printf '%s\n' off > "$MARKER.tmp.$$"
  mv "$MARKER.tmp.$$" "$MARKER"
}

stop_fallback() {
  [ -r "$STATE_DIR/daemon.pid" ] || return 0
  pid=""
  IFS= read -r pid < "$STATE_DIR/daemon.pid" 2>/dev/null || true
  [ -n "$pid" ] || { rm -f "$STATE_DIR/daemon.pid"; return 0; }
  if kill -0 "$pid" 2>/dev/null; then
    args=$(ps -p "$pid" -o args= 2>/dev/null || true)
    case "$args" in
      *"$BINARY"*" daemon"*) kill "$pid" 2>/dev/null || true ;;
      *) echo "refusing to stop reused/non-supervisor pid $pid" >&2; return 1 ;;
    esac
    count=0
    while kill -0 "$pid" 2>/dev/null && [ "$count" -lt 50 ]; do
      sleep 0.1
      count=$((count + 1))
    done
    kill -0 "$pid" 2>/dev/null && { echo "owned supervisor did not stop" >&2; return 1; }
  fi
  rm -f "$STATE_DIR/daemon.pid"
}

start_fallback() {
  if [ -r "$STATE_DIR/daemon.pid" ]; then
    pid=""
    IFS= read -r pid < "$STATE_DIR/daemon.pid" 2>/dev/null || true
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      args=$(ps -p "$pid" -o args= 2>/dev/null || true)
      case "$args" in *"$BINARY"*" daemon"*) return 0 ;; esac
      echo "refusing to replace reused/non-supervisor pid $pid" >&2
      return 1
    fi
  fi
  mkdir -p "$STATE_DIR"
  MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
  MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
    nohup "$BINARY" daemon --foreground \
      >"$STATE_DIR/daemon.out.log" 2>"$STATE_DIR/daemon.err.log" &
  printf '%s\n' "$!" > "$STATE_DIR/daemon.pid"
}

service_off() {
  case "$SERVICE_KIND" in
    systemd)
      systemctl --user disable --now memory-supervisor.service
      if systemctl --user is-active --quiet memory-supervisor.service; then
        echo "owned supervisor service is still active" >&2
        return 1
      fi
      if systemctl --user is-enabled --quiet memory-supervisor.service; then
        echo "owned supervisor service is still enabled" >&2
        return 1
      fi
      ;;
    launchd)
      launchctl disable "gui/$(id -u)/$LABEL"
      launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
      if launchctl print "gui/$(id -u)/$LABEL" >/dev/null 2>&1; then
        echo "owned supervisor launch agent is still loaded" >&2
        return 1
      fi
      ;;
    fallback) stop_fallback ;;
  esac
}

service_on() {
  case "$SERVICE_KIND" in
    systemd)
      systemctl --user daemon-reload
      systemctl --user enable memory-supervisor.service >/dev/null
      systemctl --user restart memory-supervisor.service
      ;;
    launchd)
      launchctl enable "gui/$(id -u)/$LABEL"
      launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
      launchctl bootstrap "gui/$(id -u)" "$PLIST"
      ;;
    fallback) start_fallback ;;
  esac
}

wait_fresh() {
  count=0
  while [ "$count" -lt 100 ]; do
    if MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
      MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
      "$BINARY" status --json >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
    count=$((count + 1))
  done
  return 1
}

if [ "$ACTION" = off ]; then
  write_marker
  if ! service_off; then
    rm -f "$MARKER"
    if service_on >/dev/null 2>&1 && wait_fresh; then
      echo "Memory Supervisor could not be turned off; it remains ON with fresh protection" >&2
    else
      write_marker
      service_off >/dev/null 2>&1 || true
      rm -f "$STATE_DIR/admission-green.lease"
      echo "Memory Supervisor could not restore fresh ON protection; it remains OFF" >&2
    fi
    exit 1
  fi
  rm -f "$STATE_DIR/admission-green.lease"
  echo "Memory Supervisor is OFF. Services stay disabled across restarts; installed CLI hooks pass through."
  echo "Run 'memory-supervisor on' once to restore protection."
  exit 0
fi

rm -f "$MARKER" "$STATE_DIR/state.json" "$STATE_DIR/admission-green.lease"
if ! service_on || ! wait_fresh; then
  write_marker
  service_off >/dev/null 2>&1 || true
  echo "Memory Supervisor did not return to a fresh running state; it remains off" >&2
  exit 1
fi
echo "Memory Supervisor is ON. Protection is running and stays enabled across restarts."
echo "Installed Claude Code and Codex connections remain in place."
