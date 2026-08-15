#!/bin/sh
# One-command release installer. Normal users need no Git checkout or language toolchain.
set -eu

RELEASE_BASE_URL=${MEMORY_SUPERVISOR_RELEASE_BASE_URL:-"https://github.com/lssLab/Calando/releases/latest/download"}
SOURCE=${MEMORY_SUPERVISOR_INSTALL_ROOT:-"$HOME/.local/share/memory-supervisor"}
SOURCE_MARKER=.memory-supervisor-release-source
TEMP_DIR=""
BACKUP=""
SOURCE_REPLACED=0
SUCCESS=0

case "$SOURCE" in
  ""|/|"$HOME")
    echo "refusing unsafe install root: $SOURCE" >&2
    exit 1
    ;;
esac

cleanup() {
  code=$?
  trap - EXIT HUP INT TERM
  if [ "$code" -ne 0 ] && [ "$SOURCE_REPLACED" -eq 1 ]; then
    rm -rf "$SOURCE"
    if [ -n "$BACKUP" ] && [ -e "$BACKUP" ]; then
      mv "$BACKUP" "$SOURCE"
    fi
  elif [ "$SUCCESS" -eq 1 ] && [ -n "$BACKUP" ] && [ -e "$BACKUP" ]; then
    rm -rf "$BACKUP"
  fi
  if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
    rm -rf "$TEMP_DIR"
  fi
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

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
    echo "the operating system has no SHA-256 command for release verification" >&2
    return 1
  fi
  [ "$actual" = "$expected" ]
}

# A manually cloned development checkout keeps its normal Git update path. The public one-line
# installer below never asks a user to install Git.
if [ -d "$SOURCE/.git" ]; then
  command -v git >/dev/null 2>&1 || {
    echo "this existing development checkout needs Git to update" >&2
    exit 1
  }
  git -C "$SOURCE" pull --ff-only
  SUCCESS=1
  exec /bin/sh "$SOURCE/install.sh"
fi

if [ -L "$SOURCE" ]; then
  echo "refusing to replace symlink install root: $SOURCE" >&2
  exit 1
fi
if [ -e "$SOURCE" ] && [ ! -f "$SOURCE/$SOURCE_MARKER" ]; then
  echo "refusing to replace a directory not owned by the release installer: $SOURCE" >&2
  exit 1
fi

command -v tar >/dev/null 2>&1 || {
  echo "the operating system has no archive extractor required for installation" >&2
  exit 1
}
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/memory-supervisor-bootstrap.XXXXXX")
ARCHIVE="$TEMP_DIR/memory-supervisor-source.tar.gz"
CHECKSUM="$ARCHIVE.sha256"

if [ -n "${MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE:-}" ]; then
  [ -f "$MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE" ] || {
    echo "MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE is not a file" >&2
    exit 1
  }
  [ -f "${MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE:-}" ] || {
    echo "MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE is not a file" >&2
    exit 1
  }
  cp "$MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE" "$ARCHIVE"
  cp "$MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE" "$CHECKSUM"
else
  command -v curl >/dev/null 2>&1 || {
    echo "this terminal has no HTTPS downloader; use a standard Linux, WSL2, or macOS terminal" >&2
    exit 1
  }
  curl --proto '=https' --tlsv1.2 -fL \
    "$RELEASE_BASE_URL/memory-supervisor-source.tar.gz" -o "$ARCHIVE"
  curl --proto '=https' --tlsv1.2 -fL \
    "$RELEASE_BASE_URL/memory-supervisor-source.tar.gz.sha256" -o "$CHECKSUM"
fi

verify_checksum "$ARCHIVE" "$CHECKSUM" || {
  echo "release source checksum verification failed" >&2
  exit 1
}

mkdir -p "$TEMP_DIR/extracted"
tar -xzf "$ARCHIVE" -C "$TEMP_DIR/extracted"
set -- "$TEMP_DIR/extracted"/*
if [ "$#" -ne 1 ] || [ ! -d "$1" ] || [ ! -f "$1/install.sh" ]; then
  echo "release source archive has an invalid layout" >&2
  exit 1
fi
CANDIDATE=$1

mkdir -p "$(dirname "$SOURCE")"
if [ -e "$SOURCE" ]; then
  BACKUP="$SOURCE.bootstrap-backup.$$"
  [ ! -e "$BACKUP" ] || {
    echo "temporary source backup already exists: $BACKUP" >&2
    exit 1
  }
  mv "$SOURCE" "$BACKUP"
fi
mv "$CANDIDATE" "$SOURCE"
SOURCE_REPLACED=1
printf '%s\n' "$RELEASE_BASE_URL" > "$SOURCE/$SOURCE_MARKER"
chmod 600 "$SOURCE/$SOURCE_MARKER" 2>/dev/null || true

/bin/sh "$SOURCE/install.sh"
SUCCESS=1
