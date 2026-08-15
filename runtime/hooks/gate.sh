#!/bin/sh
# Stable fail-open wrapper. Keep the fast GREEN lease and all slow-path logic in Rust.
#
# The daemon alone computes a short GREEN lease after reading fresh federation peers. A stale
# daemon, changed pointer, pressure/incident state, or unsupported event falls through to Rust.
EVENT=${1:-}
case "$EVENT" in
  claude|codex) EVENT=${2:-} ;;
esac
case "$EVENT" in
  PreToolUse|PostToolUse|PostToolBatch|AfterTool|UserPromptSubmit|BeforeAgent)
    STATE_ROOT=${MEMORY_SUPERVISOR_DIR:-}
    if [ -z "$STATE_ROOT" ] && [ -r "$HOME/.memory-supervisor/state-dir" ]; then
      IFS= read -r STATE_ROOT < "$HOME/.memory-supervisor/state-dir" 2>/dev/null || :
    fi
    [ -n "$STATE_ROOT" ] || STATE_ROOT="$HOME/.cache/memory-supervisor"
    LEASE="$STATE_ROOT/admission-green.lease"
    EXPECTED_FED=${MEMORY_SUPERVISOR_FEDERATION_DIR:-}
    if [ -z "$EXPECTED_FED" ] && [ -r "$HOME/.memory-supervisor/federation-dir" ]; then
      IFS= read -r EXPECTED_FED < "$HOME/.memory-supervisor/federation-dir" 2>/dev/null || :
    fi
    if [ -r "$LEASE" ] && [ -n "$EXPECTED_FED" ]; then
      VERSION="" EXPIRES="" LEASE_FED=""
      {
        IFS= read -r VERSION || :
        IFS= read -r EXPIRES || :
        IFS= read -r LEASE_FED || :
      } < "$LEASE"
      case "$EXPIRES" in
        ''|*[!0-9]*) ;;
        *)
          NOW=$(date +%s 2>/dev/null || printf '0')
          if [ "$VERSION" = "1" ] && [ "$LEASE_FED" = "$EXPECTED_FED" ] && \
             [ "$NOW" -lt "$EXPIRES" ]; then
            exit 0
          fi
          ;;
      esac
    fi
    ;;
esac
BINARY=${MEMORY_SUPERVISOR_BINARY:-}
if [ -z "$BINARY" ] && [ -r "$HOME/.memory-supervisor/binary" ]; then
  IFS= read -r BINARY < "$HOME/.memory-supervisor/binary" 2>/dev/null || :
fi
[ -n "$BINARY" ] || BINARY="$HOME/.local/lib/memory-supervisor/memory-supervisor"
[ -x "$BINARY" ] || exit 0
OUTPUT=$("$BINARY" gate "$@" 2>/dev/null) || exit 0
[ -n "$OUTPUT" ] && printf '%s' "$OUTPUT"
exit 0
