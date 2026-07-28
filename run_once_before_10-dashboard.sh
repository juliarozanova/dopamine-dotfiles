#!/bin/sh
# Runs once per machine on `chezmoi apply`: builds the Dashboard skeleton.
set -e
D="${DASHBOARD:-$HOME/Dashboard}"

mkdir -p "$D/Code" "$D/Work" \
         "$D/Knowledge/papers/pdf" "$D/Knowledge/hdl" "$D/Knowledge/tickets"

seed() { [ -f "$1" ] || printf '%s\n' "$2" > "$1"; }

seed "$D/Knowledge/hdl/best-practices.md" \
"# HDL best practices

Curated. One dated bullet per practice, with the *why*.
Promote entries here from inbox.md during the weekly prune."

seed "$D/Knowledge/hdl/inbox.md" \
"# HDL inbox

Raw #hdl learnings land here automatically (\`tk done\`).
Weekly five-minute prune: promote keepers → best-practices.md, delete the rest."

seed "$D/Knowledge/tickets/README.md" \
"# Ticket learnings

One file per ticket, appended by \`tk done\`. Grep me."

echo "dashboard skeleton ready at $D"
