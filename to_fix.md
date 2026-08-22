# TO DO
- want to mkdir and git init automatically on tk ticket select optionally, if my chosen folder doesn't yet exist
- dash colours


(empty — next ideas land here)

## Future direction (tk-tui)
- issue-list mode replacing the fzf picker
- status transitions from the TUI (To Do → In Progress → …)
- sprint board view
- nested checkboxes (ADF taskLists nest; the list flattens them)
- ordering/priority in the global list — grouping by ticket stops scaling
  somewhere north of ~20 open tickets

# Accepted / won't fix
1. The alternating `<` colours in the zellij bottom ribbon are hardcoded in
   zellij's status-bar plugin (alternate segments paint their brackets in the
   terminal-background colour; not exposed to themes). Stock behaviour on
   every zellij install — accepted.

# Done
1. ~~Remove the TICKET.md functionality~~ — gone; nvim opens plain, Claude
   context points at `jira issue view`, the TUI is the only Jira surface.
2. ~~Ticket pane modular~~ — `tk view [KEY] [--float]` (alias `tk tui`) runs
   the ratatui pane in any terminal, floats it in zellij, and the ticket
   layout uses the same entry point. Optional keybinding snippet in README.
3. ~~Reply + vim motions in the tk interface~~ — tk-tui: j/k, u/d, gg/G,
   J/K comment select, R quoted reply, c comment, r refresh, w web.
4. TK ticket selector vim motions (fzf ctrl-j/k etc.)
5. Zellij ribbon + tabs visible, dopamine-themed (component theme port)
6. Ctrl-hjkl freed for neovim; Alt-hjkl is zellij movement
7. lazygit stacked with neovim in the main stage
8. Functional jira ticket pane (superseded by tk-tui)
9. Dash "sprint" pane → "my issues" (team-managed projects have no sprint
   list); ~~hdl-inbox documented~~ (superseded by 10 — the inbox is gone)
10. Global todo tool — `tk todo` aggregates the `TODO` section of every open
    assigned ticket plus `~/Dashboard/todo.md` into one editable list, writes
    back to Jira surgically (the description is mutated in place, never
    re-serialised), `p` promotes a local item onto a ticket, `Alt-t` floats it
    anywhere, and the ticket pane gained a `t` TODO mode instead of a second
    pane. The hdl inbox and the whole `#hdl` pipeline are gone with it.
11. ~~ctrl s in ticket pane interferes~~ — the compose box now uses the same
    modal editor as the checklist; `ZZ` sends, so nothing asks the terminal
    for `ctrl-s` (which is XOFF flow control and never arrives).

# Parked (theming — revisit later)
7. Dash colour scheme?
8. Light/dark from the single chezmoi palette?
