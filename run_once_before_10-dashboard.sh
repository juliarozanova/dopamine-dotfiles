#!/bin/sh
# Runs once per machine on `chezmoi apply`: builds the Dashboard skeleton.
set -e
D="${DASHBOARD:-$HOME/Dashboard}"

mkdir -p "$D/Code" "$D/Work" \
         "$D/Knowledge/papers/pdf" "$D/Knowledge/tickets"

seed() { [ -f "$1" ] || printf '%s\n' "$2" > "$1"; }

seed "$D/todo.md" \
"# Todo

Work with no ticket of its own. \`tk todo\` reads the \`- [ ]\` lines here and
shows them alongside the TODO section of every open Jira ticket.
Prose and headings are yours; only the checkbox lines are ever rewritten."

seed "$D/Knowledge/tickets/README.md" \
"# Ticket learnings

One file per ticket, appended by \`tk done\`. Grep me."

echo "dashboard skeleton ready at $D"
