#!/bin/sh
# Stable compatibility wrapper. Notification failures must never block the supervisor.
BINARY=${MEMORY_SUPERVISOR_BINARY:-}
if [ -z "$BINARY" ] && [ -r "$HOME/.calando/binary" ]; then
  IFS= read -r BINARY < "$HOME/.calando/binary" 2>/dev/null || :
fi
[ -n "$BINARY" ] || BINARY="$HOME/.local/lib/calando/calando"
[ -x "$BINARY" ] || exit 0
"$BINARY" notify "${1:-Calando alert}" >/dev/null 2>&1 || true
exit 0
