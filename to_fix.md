# TO DO
- ctrl s in ticket pane interferes
- dash colours


(empty — next ideas land here)

## Future direction (tk-tui)
- issue-list mode replacing the fzf picker
- status transitions from the TUI (To Do → In Progress → …)
- sprint board view

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
   list); hdl-inbox documented (tk done harvests #hdl lines → weekly prune)

# Parked (theming — revisit later)
7. Dash colour scheme?
8. Light/dark from the single chezmoi palette?
