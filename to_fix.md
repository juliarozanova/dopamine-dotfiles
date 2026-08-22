# TO DO
- Alt T interferes with Alt F now - I still want to be able to use alt f as normal, and alt t to bring up and minimize a floating todo.
- Alt T doesn't upen the global todo, it makes a new pane which does nothing but opens a default floating pane
- t doesn't switch to todo mode in ticket pane. Also no hint for it with other hotkeys hints at the bottom.
- In the global todo pane, j k  isn't actually doing anything - doesn't seem to be moving position or highlighting sections/todo items, o only edits the first one
- O doesn't preview the thing being typed at all? I'm completely blind until it is done

# TO DO (Later, ignore for now)
- want to mkdir and git init automatically on tk ticket select optionally, if my chosen folder doesn't yet exist
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

12. ~~First cut of `tk todo` barely worked~~ — four bugs, all of them
    rendering, all missed because the tests only ever checked logic and the
    `--dump` text. Fixed, with `TestBackend` tests that draw real frames:
    - `Alt-t` opened a spare pane that did nothing. zellij's `Run` *always*
      opens a pane for its command, so the helper script's only job was to
      open a second one. Alt-t now runs `tk todo` as the floating pane itself.
    - `t` in the ticket pane flipped a field nothing rendered, and the footer
      never mentioned it.
    - The edit buffer was patched over a row after the fact, using an index
      that doesn't exist for a new item — so it drew over the next group's
      row, or on an empty list drew nowhere at all (typing blind). Editing is
      now part of building the rows.
    - `O` wasn't bound at all.

# Parked (theming — revisit later)
7. Dash colour scheme?
8. Light/dark from the single chezmoi palette?
