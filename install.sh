#!/bin/sh
# Memory Supervisor installer for Linux, WSL, and macOS. Windows uses install.ps1.
set -eu

DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
RUNTIME_DIR=${MEMORY_SUPERVISOR_RUNTIME_DIR:-"$HOME/.local/lib/memory-supervisor"}
INSTALLED_BINARY="$RUNTIME_DIR/memory-supervisor"
POINTER_DIR="$HOME/.memory-supervisor"
POWERED_OFF=0
if [ -e "$POINTER_DIR/power-off" ] || [ -L "$POINTER_DIR/power-off" ]; then POWERED_OFF=1; fi
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/memory-supervisor-install.XXXXXX")
SERVICE_KIND=""
SERVICE_FILE=""
HAD_SERVICE=0
HAD_BINARY=0
HAD_FALLBACK=0
CUTOVER=0
SUCCESS=0
ACTIVATED=0
HAD_STATE=0
OLD_FALLBACK_PYTHON=0
OLD_PYTHON_RUNTIME=0
PREVIOUS_ROOT="$DIR"
PYTHON_ROLLBACK_DIR="$TEMP_DIR/python-rollback"
ROLLBACK_PYTHON=""

python_runtime_files() {
  printf '%s\n' supervisor.py memory_supervisor_config.py memory_supervisor_events.py \
    memory_supervisor_platform.py notify/notify.py notify/terminal_notice.py
}

python_rollback_complete() {
  for file in $(python_runtime_files); do
    [ -f "$PYTHON_ROLLBACK_DIR/$file" ] || return 1
  done
}

copy_python_rollback() {
  source_root=$1
  rm -rf "$PYTHON_ROLLBACK_DIR"
  mkdir -p "$PYTHON_ROLLBACK_DIR/notify"
  for file in $(python_runtime_files); do
    [ -f "$source_root/$file" ] || return 1
    cp "$source_root/$file" "$PYTHON_ROLLBACK_DIR/$file"
  done
}

extract_python_rollback() {
  source_root=$1
  revision=$2
  rm -rf "$PYTHON_ROLLBACK_DIR"
  mkdir -p "$PYTHON_ROLLBACK_DIR/notify"
  for file in $(python_runtime_files); do
    if ! git -C "$source_root" show "$revision:$file" > "$PYTHON_ROLLBACK_DIR/$file"; then
      return 1
    fi
  done
}

prepare_python_rollback() {
  if ! copy_python_rollback "$PREVIOUS_ROOT" 2>/dev/null && \
     ! copy_python_rollback "$DIR" 2>/dev/null; then
    found=0
    if command -v git >/dev/null 2>&1; then
      for source_root in "$PREVIOUS_ROOT" "$DIR"; do
        [ -d "$source_root/.git" ] || continue
        for revision in ORIG_HEAD HEAD^; do
          if extract_python_rollback "$source_root" "$revision" 2>/dev/null && \
             python_rollback_complete; then
            found=1
            break 2
          fi
        done
      done
    fi
    [ "$found" -eq 1 ] || {
      echo "cannot preserve the running Python supervisor for rollback; installation was not changed" >&2
      return 1
    }
  fi
  python_rollback_complete || return 1
  ROLLBACK_PYTHON=$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)
  [ -n "$ROLLBACK_PYTHON" ] || {
    echo "cannot locate the interpreter used by the previous Python supervisor; installation was not changed" >&2
    return 1
  }
  OLD_PYTHON_RUNTIME=1
}

restore_python_rollback() {
  [ "$OLD_PYTHON_RUNTIME" -eq 1 ] || return 0
  mkdir -p "$PREVIOUS_ROOT/notify"
  for file in $(python_runtime_files); do
    cp "$PYTHON_ROLLBACK_DIR/$file" "$PREVIOUS_ROOT/$file"
  done
  printf '%s\n' "restored after failed Rust activation" > \
    "$PREVIOUS_ROOT/.memory-supervisor-python-rollback"
}

cleanup_python_rollback() {
  marker="$PREVIOUS_ROOT/.memory-supervisor-python-rollback"
  [ -f "$marker" ] || return 0
  for file in $(python_runtime_files); do
    rm -f "$PREVIOUS_ROOT/$file"
  done
  rmdir "$PREVIOUS_ROOT/notify" 2>/dev/null || true
  rm -f "$marker"
}

remove_legacy_service() {
  if [ "$(uname -s)" = Darwin ]; then
    legacy="$HOME/Library/LaunchAgents/com.claude.memory-governor.plist"
    if [ ! -L "$legacy" ] && [ -f "$legacy" ] && \
      grep -Fq "com.claude.memory-governor" "$legacy" && \
      grep -Fq "governor.py" "$legacy"; then
      launchctl bootout "gui/$(id -u)" "$legacy"
      rm -f "$legacy"
    fi
  elif command -v systemctl >/dev/null 2>&1 && \
    systemctl --user show-environment >/dev/null 2>&1; then
    legacy="$HOME/.config/systemd/user/claude-governor.service"
    if [ ! -L "$legacy" ] && [ -f "$legacy" ] && grep -Fq "governor.py" "$legacy"; then
      systemctl --user disable --now claude-governor.service
      rm -f "$legacy"
      systemctl --user daemon-reload
    fi
  fi
}

rollback() {
  [ "$CUTOVER" -eq 1 ] || return 0
  echo "activation failed; restoring the previous Memory Supervisor runtime" >&2
  if ! restore_python_rollback; then
    echo "warning: the emergency Python runtime could not be restored" >&2
  fi
  case "$SERVICE_KIND" in
    systemd)
      systemctl --user stop memory-supervisor.service 2>/dev/null || true
      ;;
    launchd)
      launchctl bootout "gui/$(id -u)" "$SERVICE_FILE" 2>/dev/null || true
      ;;
    fallback)
      if [ -r "$STATE_DIR/daemon.pid" ]; then
        OLD_PID=""
        IFS= read -r OLD_PID < "$STATE_DIR/daemon.pid" 2>/dev/null || true
        if [ -n "$OLD_PID" ]; then
          OLD_ARGS=$(ps -p "$OLD_PID" -o args= 2>/dev/null || true)
          case "$OLD_ARGS" in
            *"$INSTALLED_BINARY"*" daemon"*) kill "$OLD_PID" 2>/dev/null || true ;;
          esac
        fi
      fi
      ;;
  esac
  if [ "$HAD_BINARY" -eq 1 ] && [ -f "$TEMP_DIR/previous-binary" ]; then
    cp "$TEMP_DIR/previous-binary" "$INSTALLED_BINARY"
    chmod 755 "$INSTALLED_BINARY"
  elif [ "$HAD_BINARY" -eq 0 ]; then
    rm -f "$INSTALLED_BINARY"
  fi
  if [ "$HAD_STATE" -eq 1 ] && [ -f "$TEMP_DIR/previous-state.json" ]; then
    cp "$TEMP_DIR/previous-state.json" "$STATE_DIR/state.json"
  else
    rm -f "$STATE_DIR/state.json"
  fi
  case "$SERVICE_KIND" in
    systemd)
      if [ "$HAD_SERVICE" -eq 1 ]; then
        cp "$TEMP_DIR/previous-service" "$SERVICE_FILE"
        systemctl --user daemon-reload 2>/dev/null || true
        if [ "$POWERED_OFF" -eq 1 ]; then
          systemctl --user disable --now memory-supervisor.service 2>/dev/null || true
        else
          systemctl --user enable --now memory-supervisor.service 2>/dev/null || true
        fi
      else
        rm -f "$SERVICE_FILE"
        systemctl --user daemon-reload 2>/dev/null || true
      fi
      ;;
    launchd)
      if [ "$HAD_SERVICE" -eq 1 ]; then
        cp "$TEMP_DIR/previous-service" "$SERVICE_FILE"
        if [ "$POWERED_OFF" -eq 1 ]; then
          launchctl disable "gui/$(id -u)/io.github.lsslab.memory-supervisor" 2>/dev/null || true
        else
          launchctl enable "gui/$(id -u)/io.github.lsslab.memory-supervisor" 2>/dev/null || true
          launchctl bootstrap "gui/$(id -u)" "$SERVICE_FILE" 2>/dev/null || true
        fi
      else
        rm -f "$SERVICE_FILE"
      fi
      ;;
    fallback)
      if [ "$HAD_FALLBACK" -eq 1 ] && [ "$OLD_FALLBACK_PYTHON" -eq 1 ] && \
        [ -f "$PREVIOUS_ROOT/supervisor.py" ]; then
        if [ -n "$ROLLBACK_PYTHON" ]; then
          MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
          MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
            nohup "$ROLLBACK_PYTHON" "$PREVIOUS_ROOT/supervisor.py" \
              >"$STATE_DIR/daemon.out.log" 2>"$STATE_DIR/daemon.err.log" &
          printf '%s\n' "$!" > "$STATE_DIR/daemon.pid"
        fi
      elif [ "$POWERED_OFF" -eq 0 ] && [ "$HAD_FALLBACK" -eq 1 ] && [ "$HAD_BINARY" -eq 1 ]; then
        MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
        MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
          nohup "$INSTALLED_BINARY" daemon --foreground \
            >"$STATE_DIR/daemon.out.log" 2>"$STATE_DIR/daemon.err.log" &
        printf '%s\n' "$!" > "$STATE_DIR/daemon.pid"
      fi
      ;;
  esac
}

finish() {
  code=$?
  trap - EXIT HUP INT TERM
  if [ "$code" -ne 0 ] && [ "$SUCCESS" -ne 1 ] && [ "$ACTIVATED" -ne 1 ]; then
    rollback
  elif [ "$code" -ne 0 ] && [ "$ACTIVATED" -eq 1 ]; then
    echo "Rust runtime is active, but post-activation setup was incomplete; rerun install.sh" >&2
  fi
  rm -rf "$TEMP_DIR"
  exit "$code"
}
trap finish EXIT
trap 'exit 130' HUP INT TERM

release_target() {
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Linux:x86_64|Linux:amd64) printf '%s\n' x86_64-unknown-linux-gnu ;;
    Darwin:x86_64|Darwin:amd64) printf '%s\n' x86_64-apple-darwin ;;
    Darwin:arm64|Darwin:aarch64) printf '%s\n' aarch64-apple-darwin ;;
    *) return 1 ;;
  esac
}

verify_checksum() {
  file=$1
  checksum_file=$2
  expected=$(awk 'NR == 1 { print $1 }' "$checksum_file")
  [ -n "$expected" ] || return 1
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$file" | awk '{ print $1 }')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$file" | awk '{ print $1 }')
  else
    echo "SHA-256 tool is required to verify the release binary" >&2
    return 1
  fi
  [ "$actual" = "$expected" ]
}

obtain_binary() {
  if [ -n "${MEMORY_SUPERVISOR_BINARY_SOURCE:-}" ]; then
    [ -x "$MEMORY_SUPERVISOR_BINARY_SOURCE" ] || {
      echo "MEMORY_SUPERVISOR_BINARY_SOURCE is not executable" >&2
      return 1
    }
    printf '%s\n' "$MEMORY_SUPERVISOR_BINARY_SOURCE"
    return
  fi
  if command -v cargo >/dev/null 2>&1 && [ -f "$DIR/Cargo.toml" ] && \
    [ ! -f "$DIR/.memory-supervisor-release-source" ]; then
    echo "building the current Memory Supervisor source with Rust" >&2
    cargo build --manifest-path "$DIR/Cargo.toml" --release --locked >&2
    printf '%s\n' "$DIR/target/release/memory-supervisor"
    return
  fi
  target=$(release_target) || {
    echo "no release binary is available for $(uname -s)/$(uname -m)" >&2
    return 1
  }
  command -v curl >/dev/null 2>&1 || {
    echo "curl is required to download the verified release binary" >&2
    return 1
  }
  asset="memory-supervisor-$target"
  base=${MEMORY_SUPERVISOR_RELEASE_BASE_URL:-"https://github.com/lssLab/claude-code-codex-memory-supervisor-prerelease/releases/latest/download"}
  echo "downloading verified release binary: $asset" >&2
  curl -fL "$base/$asset" -o "$TEMP_DIR/$asset" >&2
  curl -fL "$base/$asset.sha256" -o "$TEMP_DIR/$asset.sha256" >&2
  verify_checksum "$TEMP_DIR/$asset" "$TEMP_DIR/$asset.sha256" || {
    echo "release checksum verification failed" >&2
    return 1
  }
  chmod 755 "$TEMP_DIR/$asset"
  printf '%s\n' "$TEMP_DIR/$asset"
}

stop_fallback() {
  [ -r "$STATE_DIR/daemon.pid" ] || return 0
  pid=""
  IFS= read -r pid < "$STATE_DIR/daemon.pid" 2>/dev/null || true
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null || return 0
  args=$(ps -p "$pid" -o args= 2>/dev/null || true)
  case "$args" in
    *"$PREVIOUS_ROOT/supervisor.py"*|*"$DIR/supervisor.py"*)
      prepare_python_rollback
      OLD_FALLBACK_PYTHON=1
      ;;
    *"$INSTALLED_BINARY"*" daemon"*) ;;
    *) echo "refusing to signal reused/non-supervisor fallback pid $pid" >&2; return 1 ;;
  esac
  HAD_FALLBACK=1
  CUTOVER=1
  kill "$pid"
  count=0
  while kill -0 "$pid" 2>/dev/null && [ "$count" -lt 50 ]; do
    sleep 0.1
    count=$((count + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    echo "owned supervisor did not stop; installation aborted without force-kill" >&2
    return 1
  fi
  rm -f "$STATE_DIR/daemon.pid"
}

echo "[1/7] acquire and verify Rust runtime"
for file in "$DIR/bootstrap.sh" "$DIR/install.sh" "$DIR/uninstall.sh" "$DIR/power.sh" \
  "$DIR/hooks/gate.sh" "$DIR/notify/notify.sh" "$DIR/bin/memory-status" \
  "$DIR/bin/memory-supervisor"; do
  /bin/sh -n "$file"
done
CANDIDATE=$(obtain_binary)
"$CANDIDATE" --version

# Resolve an existing state path before stopping the old service.
if [ -n "${MEMORY_SUPERVISOR_DIR:-}" ]; then
  STATE_DIR=$MEMORY_SUPERVISOR_DIR
elif [ -r "$POINTER_DIR/state-dir" ]; then
  STATE_DIR=""
  IFS= read -r STATE_DIR < "$POINTER_DIR/state-dir" 2>/dev/null || true
  [ -n "$STATE_DIR" ] || STATE_DIR="$HOME/.cache/memory-supervisor"
elif [ -r "$HOME/.memory-governor/state-dir" ]; then
  STATE_DIR=""
  IFS= read -r STATE_DIR < "$HOME/.memory-governor/state-dir" 2>/dev/null || true
  [ -n "$STATE_DIR" ] || STATE_DIR="$HOME/.cache/claude-governor"
else
  STATE_DIR="$HOME/.cache/memory-supervisor"
fi
FEDERATION_DIR=${MEMORY_SUPERVISOR_FEDERATION_DIR:-"$HOME/.memory-supervisor/instances"}
if [ -r "$POINTER_DIR/install-root" ]; then
  PREVIOUS_ROOT=""
  IFS= read -r PREVIOUS_ROOT < "$POINTER_DIR/install-root" 2>/dev/null || true
  [ -n "$PREVIOUS_ROOT" ] || PREVIOUS_ROOT="$DIR"
fi

echo "[2/7] preserve state and switch the owned daemon"
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

mkdir -p "$RUNTIME_DIR"
if [ -f "$INSTALLED_BINARY" ]; then
  cp "$INSTALLED_BINARY" "$TEMP_DIR/previous-binary"
  HAD_BINARY=1
fi

case "$SERVICE_KIND" in
  systemd)
    SERVICE_FILE="$HOME/.config/systemd/user/memory-supervisor.service"
    if [ -L "$SERVICE_FILE" ]; then
      echo "refusing to replace user-service link: $SERVICE_FILE" >&2
      exit 1
    elif [ -f "$SERVICE_FILE" ]; then
      if ! grep -Fq "Claude Code & Codex CLI Memory Supervisor" "$SERVICE_FILE" && \
         ! { grep -Fq "$PREVIOUS_ROOT/supervisor.py" "$SERVICE_FILE" && \
             grep -Fq "MEMORY_SUPERVISOR_DIR" "$SERVICE_FILE"; }; then
        echo "refusing to replace foreign user service: $SERVICE_FILE" >&2
        exit 1
      fi
      cp "$SERVICE_FILE" "$TEMP_DIR/previous-service"
      HAD_SERVICE=1
      if grep -Fq "supervisor.py" "$SERVICE_FILE"; then
        prepare_python_rollback
      fi
      CUTOVER=1
      systemctl --user stop memory-supervisor.service 2>/dev/null || true
    fi
    ;;
  launchd)
    SERVICE_FILE="$HOME/Library/LaunchAgents/io.github.lsslab.memory-supervisor.plist"
    if [ -L "$SERVICE_FILE" ]; then
      echo "refusing to replace launch-agent link: $SERVICE_FILE" >&2
      exit 1
    elif [ -f "$SERVICE_FILE" ]; then
      grep -Fq "io.github.lsslab.memory-supervisor" "$SERVICE_FILE" || {
        echo "refusing to replace foreign launch agent: $SERVICE_FILE" >&2
        exit 1
      }
      cp "$SERVICE_FILE" "$TEMP_DIR/previous-service"
      HAD_SERVICE=1
      if grep -Fq "supervisor.py" "$SERVICE_FILE"; then
        prepare_python_rollback
      fi
      CUTOVER=1
      launchctl bootout "gui/$(id -u)" "$SERVICE_FILE" 2>/dev/null || true
    fi
    ;;
  fallback) stop_fallback ;;
esac
[ "$CUTOVER" -eq 1 ] || CUTOVER=1
if [ -f "$STATE_DIR/state.json" ]; then
  cp "$STATE_DIR/state.json" "$TEMP_DIR/previous-state.json"
  HAD_STATE=1
fi
rm -f "$STATE_DIR/state.json" "$STATE_DIR/admission-green.lease"
cp "$CANDIDATE" "$RUNTIME_DIR/.memory-supervisor.new.$$"
chmod 755 "$RUNTIME_DIR/.memory-supervisor.new.$$"
mv "$RUNTIME_DIR/.memory-supervisor.new.$$" "$INSTALLED_BINARY"

"$INSTALLED_BINARY" integration migrate-names
if [ -n "${MEMORY_SUPERVISOR_DIR:-}" ]; then
  STATE_DIR=$(MEMORY_SUPERVISOR_DIR="$MEMORY_SUPERVISOR_DIR" \
    "$INSTALLED_BINARY" integration path state)
else
  STATE_DIR=$("$INSTALLED_BINARY" integration path state)
fi
if [ -n "${MEMORY_SUPERVISOR_FEDERATION_DIR:-}" ]; then
  FEDERATION_DIR=$(MEMORY_SUPERVISOR_FEDERATION_DIR="$MEMORY_SUPERVISOR_FEDERATION_DIR" \
    "$INSTALLED_BINARY" integration path federation)
else
  FEDERATION_DIR=$("$INSTALLED_BINARY" integration path federation)
fi
mkdir -p "$POINTER_DIR" "$STATE_DIR" "$FEDERATION_DIR" "$HOME/.config/memory-supervisor"
chmod 700 "$POINTER_DIR" "$STATE_DIR" 2>/dev/null || true

systemd_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/%/%%/g'
}
xml_escape() {
  printf '%s' "$1" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g'
}

case "$SERVICE_KIND" in
  systemd)
    mkdir -p "$(dirname "$SERVICE_FILE")"
    BIN_ESC=$(systemd_escape "$INSTALLED_BINARY")
    STATE_ESC=$(systemd_escape "$STATE_DIR")
    FED_ESC=$(systemd_escape "$FEDERATION_DIR")
    cat > "$SERVICE_FILE.tmp" <<EOF
[Unit]
Description=Claude Code & Codex CLI Memory Supervisor

[Service]
ExecStart="$BIN_ESC" daemon --foreground
Environment="MEMORY_SUPERVISOR_DIR=$STATE_ESC"
Environment="MEMORY_SUPERVISOR_FEDERATION_DIR=$FED_ESC"
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
EOF
    mv "$SERVICE_FILE.tmp" "$SERVICE_FILE"
    systemctl --user daemon-reload
    if [ "$POWERED_OFF" -eq 1 ]; then
      systemctl --user disable --now memory-supervisor.service >/dev/null
      if systemctl --user is-active --quiet memory-supervisor.service ||
        systemctl --user is-enabled --quiet memory-supervisor.service; then
        echo "power state is OFF but the user service remains active or enabled" >&2
        exit 1
      fi
    else
      systemctl --user enable memory-supervisor.service >/dev/null
      systemctl --user restart memory-supervisor.service
    fi
    if [ "$POWERED_OFF" -eq 0 ] && command -v loginctl >/dev/null 2>&1 && \
      [ "$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null || true)" != yes ] && \
      loginctl enable-linger "$(id -un)" 2>/dev/null; then
      touch "$STATE_DIR/linger-enabled-by-install"
      echo "boot coverage: loginctl enable-linger $(id -un) enabled"
    fi
    ;;
  launchd)
    mkdir -p "$(dirname "$SERVICE_FILE")"
    BIN_XML=$(xml_escape "$INSTALLED_BINARY")
    STATE_XML=$(xml_escape "$STATE_DIR")
    FED_XML=$(xml_escape "$FEDERATION_DIR")
    OUT_XML=$(xml_escape "$STATE_DIR/daemon.out.log")
    ERR_XML=$(xml_escape "$STATE_DIR/daemon.err.log")
    cat > "$SERVICE_FILE.tmp" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>io.github.lsslab.memory-supervisor</string>
<key>ProgramArguments</key><array><string>$BIN_XML</string><string>daemon</string><string>--foreground</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>EnvironmentVariables</key><dict>
<key>MEMORY_SUPERVISOR_DIR</key><string>$STATE_XML</string>
<key>MEMORY_SUPERVISOR_FEDERATION_DIR</key><string>$FED_XML</string>
</dict>
<key>StandardOutPath</key><string>$OUT_XML</string>
<key>StandardErrorPath</key><string>$ERR_XML</string>
</dict></plist>
EOF
    mv "$SERVICE_FILE.tmp" "$SERVICE_FILE"
    if [ "$POWERED_OFF" -eq 1 ]; then
      launchctl disable "gui/$(id -u)/io.github.lsslab.memory-supervisor"
      launchctl bootout "gui/$(id -u)" "$SERVICE_FILE" 2>/dev/null || true
      if launchctl print "gui/$(id -u)/io.github.lsslab.memory-supervisor" >/dev/null 2>&1; then
        echo "power state is OFF but the launch agent remains loaded" >&2
        exit 1
      fi
    else
      launchctl enable "gui/$(id -u)/io.github.lsslab.memory-supervisor" 2>/dev/null || true
      launchctl bootstrap "gui/$(id -u)" "$SERVICE_FILE"
    fi
    ;;
  fallback)
    if [ "$POWERED_OFF" -eq 0 ]; then
      MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
      MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
        nohup "$INSTALLED_BINARY" daemon --foreground \
          >"$STATE_DIR/daemon.out.log" 2>"$STATE_DIR/daemon.err.log" &
      printf '%s\n' "$!" > "$STATE_DIR/daemon.pid"
    else
      rm -f "$STATE_DIR/daemon.pid"
    fi
    ;;
esac

echo "[3/7] verify the new Rust daemon before changing hooks or commands"
if [ "$POWERED_OFF" -eq 1 ]; then
  echo "power state is OFF; daemon activation remains disabled"
else
  count=0
  while [ ! -s "$STATE_DIR/state.json" ]; do
    count=$((count + 1))
    [ "$count" -lt 200 ] || {
      echo "supervisor did not publish a fresh state; inspect $STATE_DIR/daemon.err.log" >&2
      [ ! -s "$STATE_DIR/daemon.err.log" ] || sed -n '1,80p' "$STATE_DIR/daemon.err.log" >&2
      exit 1
    }
    sleep 0.1
  done
  MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
  MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
    "$INSTALLED_BINARY" status --json >/dev/null || {
      echo "supervisor published state but failed activation health checks" >&2
      MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
      MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
        "$INSTALLED_BINARY" status --json >&2 || true
      [ ! -s "$STATE_DIR/daemon.err.log" ] || sed -n '1,80p' "$STATE_DIR/daemon.err.log" >&2
      exit 1
    }
  ACTIVATED=1
fi
remove_legacy_service
cleanup_python_rollback
printf '%s\n' "$STATE_DIR" > "$POINTER_DIR/state-dir"
printf '%s\n' "$FEDERATION_DIR" > "$POINTER_DIR/federation-dir"
printf '%s\n' "$INSTALLED_BINARY" > "$POINTER_DIR/binary"
printf '%s\n' "$DIR" > "$POINTER_DIR/install-root"
chmod 600 "$POINTER_DIR/state-dir" "$POINTER_DIR/federation-dir" \
  "$POINTER_DIR/binary" "$POINTER_DIR/install-root" 2>/dev/null || true

echo "[4/7] connect supported Claude Code and Codex hooks"
# Record whether the merge actually changed a provider's hook wiring, so the
# summary can tell the user the exact truth: a binary-only update needs no new
# session, a wiring change does.  The merge prints "updated: <file>" when it
# rewrote the owned hooks and "unchanged: <file>" when they were identical.
HOOK_WIRING_CHANGED=""
connect_provider() {
  provider=$1
  command_name=$2
  target=$3
  requirement=$4
  command_path=""
  if [ "$provider" = claude ]; then
    command_path=$("$INSTALLED_BINARY" integration resolve-claude 2>/dev/null || true)
  elif command -v "$command_name" >/dev/null 2>&1; then
    command_path=$(command -v "$command_name")
  fi
  if [ -n "$command_path" ]; then
    if "$INSTALLED_BINARY" integration "check-$provider" --command "$command_path"; then
      merge=$("$INSTALLED_BINARY" integration hooks --target "$target" --provider "$provider" \
        --binary "$INSTALLED_BINARY")
      printf '%s\n' "$merge"
      case "$merge" in
        *"updated: "*) HOOK_WIRING_CHANGED="$HOOK_WIRING_CHANGED $provider" ;;
      esac
      "$INSTALLED_BINARY" integration hooks --target "$target" --provider "$provider" \
        --binary "$INSTALLED_BINARY" --check
      return
    fi
  fi
  if [ "$provider" != claude ] && [ -f "$target" ]; then
    "$INSTALLED_BINARY" integration hooks --target "$target" --provider "$provider" \
      --binary "$INSTALLED_BINARY" --remove
  fi
  if [ "$provider" = claude ]; then
    echo "$provider integration skipped: $requirement; any existing Memory Supervisor hook was preserved" >&2
  else
    echo "$provider integration skipped: $requirement" >&2
  fi
}
if [ -d "$HOME/.claude" ] || \
  "$INSTALLED_BINARY" integration resolve-claude >/dev/null 2>&1; then
  connect_provider claude claude "$HOME/.claude/settings.json" "Claude Code 2.1.217+ is required"
fi
CODEX_CONFIG_HOME=${CODEX_HOME:-"$HOME/.codex"}
CODEX_HOOK_TARGET="$CODEX_CONFIG_HOME/hooks.json"
DEFAULT_CODEX_HOOK_TARGET="$HOME/.codex/hooks.json"
if command -v codex >/dev/null 2>&1 || [ -d "$CODEX_CONFIG_HOME" ]; then
  connect_provider codex codex "$CODEX_HOOK_TARGET" \
    "Codex 0.145.0+ with stable hooks enabled is required"
fi
# Older releases always wrote ~/.codex even when Codex was using a different CODEX_HOME. Keep an
# already-installed standalone CLI route current so it gains source ownership, but never create a
# second route merely because the default path exists.
if [ "$DEFAULT_CODEX_HOOK_TARGET" != "$CODEX_HOOK_TARGET" ] && \
  [ -f "$DEFAULT_CODEX_HOOK_TARGET" ] && \
  grep -Eq 'memory-supervisor|hooks[/\\]gate' "$DEFAULT_CODEX_HOOK_TARGET"; then
  legacy_merge=$("$INSTALLED_BINARY" integration hooks --target "$DEFAULT_CODEX_HOOK_TARGET" \
    --provider codex --binary "$INSTALLED_BINARY")
  printf '%s\n' "$legacy_merge"
  case "$legacy_merge" in
    *"updated: "*) HOOK_WIRING_CHANGED="$HOOK_WIRING_CHANGED codex" ;;
  esac
  "$INSTALLED_BINARY" integration hooks --target "$DEFAULT_CODEX_HOOK_TARGET" \
    --provider codex --binary "$INSTALLED_BINARY" --check
fi
if [ -n "$HOOK_WIRING_CHANGED" ]; then
  echo "Hook wiring changed for:${HOOK_WIRING_CHANGED}. Follow the provider-specific reload and trust steps below."
  case "$HOOK_WIRING_CHANGED" in
    *claude*) echo "Claude Code: USER ACTION REQUIRED for an untrusted workspace. There is no per-hook approval, but interactive Claude holds every settings-file hook until the user accepts workspace trust for the current folder or a parent. Accept only a folder you trust. /hooks is read-only. An already-trusted running session normally reloads the User Settings hook; restart only if it does not appear." ;;
  esac
  case "$HOOK_WIRING_CHANGED" in
    *codex*)
      echo "Codex CLI: USER ACTION REQUIRED. Open /hooks in the CLI you are using. Confirm that all seven Memory Supervisor entries are trusted and on; trust only entries marked for review and enable only entries that are off. Then close /hooks and continue the current work. Restarting does not grant trust."
      echo "Codex App: USER ACTION REQUIRED. The user must personally use Settings > Hooks, not /hooks, to trust every new or changed Memory Supervisor entry and enable any disabled entry. Restarting cannot grant trust; applying the change reloads every existing task currently loaded by the shared App Server, so continue an existing task with its next request."
      echo "Shared CODEX_HOME edge case: if another process already saved approval and this running surface has no change left to save, restart only that pre-existing App or CLI once so it reads the shared trust record."
      ;;
  esac
else
  echo "Hook wiring unchanged: a binary-only update does not create new Codex trust. Run memory-status --connections anyway; any existing Codex disabled or untrusted state still requires the user action it reports."
fi

echo "[5/7] install skills and user commands"
is_supervisor_skill() {
  [ -r "$1/SKILL.md" ] && grep -Eq '^name: (memory-governor|memory-supervisor)$' "$1/SKILL.md"
}
link_skill() {
  target=$1
  mkdir -p "$(dirname "$target")"
  if [ -L "$target" ]; then
    if [ "$(readlink "$target")" = "$DIR" ]; then return; fi
    if is_supervisor_skill "$target"; then rm -f "$target"; else
      echo "preserving existing foreign skill link: $target" >&2; return
    fi
  elif [ -e "$target" ]; then
    echo "preserving existing non-link skill: $target" >&2
    return
  fi
  ln -s "$DIR" "$target"
}
for old in "$HOME/.claude/skills/memory-governor" \
  "$HOME/.agents/skills/memory-governor" "$HOME/.codex/skills/memory-governor"; do
  if [ -L "$old" ] && is_supervisor_skill "$old"; then rm -f "$old"; fi
done
link_skill "$HOME/.claude/skills/memory-supervisor"
link_skill "$HOME/.agents/skills/memory-supervisor"
if [ -d "$HOME/.codex" ]; then link_skill "$HOME/.codex/skills/memory-supervisor"; fi

install_command_file() {
  source=$1
  target=$2
  mkdir -p "$(dirname "$target")"
  if [ -L "$target" ] || [ -d "$target" ]; then
    echo "preserving existing non-regular command path: $target" >&2
  elif [ -f "$target" ] && ! cmp -s "$target" "$source"; then
    echo "preserving customized command file: $target" >&2
  elif [ ! -e "$target" ]; then
    cp "$source" "$target"
  fi
}
install_command_file "$DIR/commands/claude/memory-status.md" \
  "$HOME/.claude/commands/memory-status.md"
if [ -d "$HOME/.codex" ]; then
  install_command_file "$DIR/commands/codex/memory-status.md" \
    "$HOME/.codex/prompts/memory-status.md"
fi

mkdir -p "$HOME/.local/bin"
link_binary() {
  target=$1
  if [ -L "$target" ]; then
    linked=$(readlink "$target")
    case "$linked" in
      "$INSTALLED_BINARY"|"$DIR/bin/memory-supervisor"|"$DIR/bin/memory-status"|"$DIR/bin/memory-control")
        rm -f "$target" ;;
      *)
        case "$linked" in
          /*) linked_root=$(dirname "$(dirname "$linked")") ;;
          *) linked_root=$(dirname "$target")/$(dirname "$(dirname "$linked")") ;;
        esac
        if is_supervisor_skill "$linked_root"; then
          rm -f "$target"
        else
          echo "preserving existing foreign command link: $target -> $linked" >&2
          return
        fi
        ;;
    esac
  elif [ -e "$target" ]; then
    echo "preserving existing foreign command: $target" >&2
    return
  fi
  ln -s "$INSTALLED_BINARY" "$target"
}
link_binary "$HOME/.local/bin/memory-supervisor"
link_binary "$HOME/.local/bin/memory-status"
for legacy in "$HOME/.local/bin/memory-control" "$HOME/.local/bin/cmg-status" \
  "$HOME/.local/bin/cmg-control" "$HOME/.local/bin/codex-governed"; do
  if [ -L "$legacy" ]; then
    linked=$(readlink "$legacy")
    if [ "$linked" = "$INSTALLED_BINARY" ]; then
      rm -f "$legacy"
      continue
    fi
    case "$linked" in
      /*) linked_root=$(dirname "$(dirname "$linked")") ;;
      *) linked_root=$(dirname "$legacy")/$(dirname "$(dirname "$linked")") ;;
    esac
    if is_supervisor_skill "$linked_root"; then rm -f "$legacy"; fi
  fi
done

echo "[6/7] preserve private notification settings"
if [ ! -f "$HOME/.config/memory-supervisor/notifications.conf" ]; then
  cp "$DIR/notify/notifications.conf.example" \
    "$HOME/.config/memory-supervisor/notifications.conf"
fi
chmod 600 "$HOME/.config/memory-supervisor/notifications.conf" 2>/dev/null || true
command -v curl >/dev/null 2>&1 || \
  echo "note: install curl before enabling Discord or Telegram notifications" >&2

echo "[7/7] verify connections"
MEMORY_SUPERVISOR_DIR="$STATE_DIR" \
MEMORY_SUPERVISOR_FEDERATION_DIR="$FEDERATION_DIR" \
  "$INSTALLED_BINARY" status --connections
SUCCESS=1
if [ "$POWERED_OFF" -eq 1 ]; then
  echo "OK. Memory Supervisor remains OFF across updates and restarts; run 'memory-supervisor on' once to restore protection."
else
  echo "OK. Ensure $HOME/.local/bin is on PATH. Run memory-status --connections. If user action is reported, Codex CLI uses /hooks, Codex App uses Settings -> Hooks, and interactive Claude requires workspace trust before settings-file hooks run."
fi
