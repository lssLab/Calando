#!/bin/sh
# Verify the complete public-install asset set after downloading the three GitHub Actions artifact groups.
set -eu

DIRECTORY=${1:-dist}
for name in \
  memory-supervisor-source.tar.gz \
  memory-supervisor-source.zip \
  memory-supervisor-x86_64-unknown-linux-gnu \
  memory-supervisor-x86_64-pc-windows-msvc.exe \
  memory-supervisor-x86_64-apple-darwin \
  memory-supervisor-aarch64-apple-darwin; do
  [ -f "$DIRECTORY/$name" ] || {
    echo "missing release asset: $name" >&2
    exit 1
  }
  [ -f "$DIRECTORY/$name.sha256" ] || {
    echo "missing release checksum: $name.sha256" >&2
    exit 1
  }
done

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$DIRECTORY" && sha256sum -c -- *.sha256)
elif command -v shasum >/dev/null 2>&1; then
  for checksum in "$DIRECTORY"/*.sha256; do
    expected=$(awk 'NR == 1 { print $1 }' "$checksum")
    name=$(awk 'NR == 1 { print $2 }' "$checksum" | sed 's/^\*//')
    actual=$(shasum -a 256 "$DIRECTORY/$name" | awk '{ print $1 }')
    [ "$actual" = "$expected" ] || {
      echo "checksum mismatch: $name" >&2
      exit 1
    }
  done
else
  echo "SHA-256 command is required to verify release assets" >&2
  exit 1
fi
