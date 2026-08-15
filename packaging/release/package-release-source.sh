#!/bin/sh
# Build the version-matched source bundles consumed by the public bootstrap scripts.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUTPUT=${1:-"$ROOT/dist"}
REVISION=${2:-HEAD}
mkdir -p "$OUTPUT"

TAR_NAME=memory-supervisor-source.tar.gz
ZIP_NAME=memory-supervisor-source.zip
git -C "$ROOT" archive --format=tar.gz --prefix=memory-supervisor/ \
  --output="$OUTPUT/$TAR_NAME" "$REVISION"
git -C "$ROOT" archive --format=zip --prefix=memory-supervisor/ \
  --output="$OUTPUT/$ZIP_NAME" "$REVISION"

write_checksum() {
  file=$1
  name=$(basename "$file")
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$OUTPUT" && sha256sum "$name" > "$name.sha256")
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$OUTPUT" && shasum -a 256 "$name" > "$name.sha256")
  else
    echo "SHA-256 command is required to package a release" >&2
    exit 1
  fi
}

write_checksum "$OUTPUT/$TAR_NAME"
write_checksum "$OUTPUT/$ZIP_NAME"
