#!/bin/sh
# Stable compatibility wrapper. Notification failures must never block the supervisor.
BINARY=${MEMORY_SUPERVISOR_BINARY:-}
if [ -z "$BINARY" ] && [ -r "$HOME/.memory-supervisor/binary" ]; then
  IFS= read -r BINARY < "$HOME/.memory-supervisor/binary" 2>/dev/null || :
fi
[ -n "$BINARY" ] || BINARY="$HOME/.local/lib/memory-supervisor/memory-supervisor"
[ -x "$BINARY" ] || exit 0
"$BINARY" notify "${1:-Claude Code & Codex CLI Memory Supervisor alert}" >/dev/null 2>&1 || true
exit 0
