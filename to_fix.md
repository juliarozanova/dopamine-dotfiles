# TO DO

(empty)

# TO DO (Later, ignore for now)
- want to mkdir and git init automatically on tk ticket select optionally, if my chosen folder doesn't yet exist
- ctrl s in ticket pane interferes
- dash colours



(empty — next ideas land here)

## Future direction (tk-tui)
- issue-list mode replacing the fzf picker
- status transitions from the TUI (To Do → In Progress → …)
- sprint board view
- `Alt-t` from a session with no ticket layout (a bare `zellij`) has nothing to
  toggle. zellij can't run a command from a keybinding without opening a pane
  for it, so there's no way to create-or-focus on demand; the pane has to be
  declared up front.

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

10. ~~First cut of `tk todo` barely worked~~ — five bugs, all in the parts no
    test ever exercised: the tests checked logic and `--dump` text and never
    drew a frame or opened a session. Fixed, with `TestBackend` tests that
    render real buffers, and layout changes verified in a throwaway session:
    - `Alt-t` opened a spare pane that did nothing, and its leftovers made
      `Alt-f` useless. zellij's `Run` opens a *new* pane every time, so the
      helper script's only job was to open a second one — and closing those
      panes made zellij re-apply the swap layout and respawn them, so they
      multiplied. The checklist is now declared as a floating pane in the
      ticket layout and `Alt-t` is plain `ToggleFloatingPanes`: it shows and
      hides one pane and never creates anything. `Alt-f` is untouched.
    - `t` in the ticket pane flipped a field nothing rendered, and the footer
      never mentioned it.
    - The edit buffer was patched over a row after the rows were built, at an
      index that doesn't exist yet for a new item — so it drew over the next
      group's row, or on an empty list drew nowhere at all (typing blind).
      Editing is now an input to building the rows.

11. ~~Couldn't add items to a ticket with no checkboxes yet~~ — `j` walked
    only *items*, so a ticket group with none was unreachable and `o` fell
    back to group 0: everything landed under "no ticket". An empty group is
    now a cursor position in its own right, so j/k/J/K reach it and `o` adds
    to that ticket, syncing to the description. Item-only operations (space,
    dd, p) decline politely there instead of acting on index 0.

12. ~~⏎ on a ticket should open the ticket pane~~ — it opened that ticket's
    checklist instead, and only from an item, so a ticket with no checkboxes
    couldn't be opened at all. ⏎ now opens the ticket pane proper from either
    an item or a group heading, `t` still swaps to its checklist, and `esc`
    steps back to the global list. `q` closes the pane from anywhere — those
    two meanings used to share one key.

15. ~~`/` did nothing~~ — not a bug in `/`. `~/.local/bin/tk-tui` was two days
    old: `cargo build` writes to `target/`, and only `chezmoi apply` runs the
    `cargo install` that puts it on PATH. A stale binary is indistinguishable
    from a broken feature, so `tk doctor` now compares the installed binary
    against the chezmoi source and says `✗ tk-tui is older than its source`.

14. `/` hands the checklist to fzf and jumps to what you pick, instead of the
    hand-rolled subsequence matcher that preceded it. fzf's engine, fzf's
    query language, and the same `ctrl-j/k` binds as tk's other pickers —
    ~180 lines of matching code deleted rather than written. The lines fzf
    gets are built from the same `selectables()` the cursor walks, so an index
    means the same thing on both sides; that is what a test pins. `h` toggles
    to open-only on the same principle: a view setting, invisible to both
    backends, with the picker following it so you can't jump to a hidden item.

13. Indented todos — items carry a depth, rendered as indentation, with
    `>>`/`<<` to indent and outdent, and `V` for a visual-line selection that
    `j`/`k` drag and a single `>` shifts wholesale — vim's own split, where an
    operator doubles in normal mode but fires once on a selection. Native in both formats: ADF task lists
    nest (verified three levels deep against real Jira) and markdown indents
    two spaces per level. Outdenting takes an item's children with it, and a
    list left childless by a move is dropped, since ADF forbids those.

# Parked (theming — revisit later)
7. Dash colour scheme?
8. Light/dark from the single chezmoi palette?
