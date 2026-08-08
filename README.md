# dopamine-dotfiles ✨

One Jira ticket = one git worktree = one zellij session = one Claude context.

```
~/Dashboard/
├── Code/         main checkouts (clone your repos here)
├── Work/         per-ticket worktrees (tk manages these)
└── Knowledge/
    ├── papers/   `paper <arxiv-url>` drops notes + PDFs here
    ├── hdl/      best-practices.md ← weekly prune ← inbox.md ← tk done
    └── tickets/  per-ticket learnings, appended by tk done
```

## Tonight (machine with this folder)

```sh
cd dopamine-dotfiles
gh repo create dopamine-dotfiles --private --source=. --push
```

## Tomorrow morning (laptop)

```sh
sh -c "$(curl -fsLS get.chezmoi.io)" -- init --apply <you>/dopamine-dotfiles
tk doctor        # shows what's left to install
jira init        # jira-cli auth: pick "API token", paste a Jira PAT
```

(or clone and run `./install.sh`.) Zellij chrome is the stock built-in
tab-bar + status-bar, coloured by the dopamine theme — no plugins to fetch.

### Dependencies

Required: `git nvim zellij jira fzf jq lazygit claude gettext` (envsubst),
and `rust` (cargo — builds `tk-tui` on `chezmoi apply`).
Delightful: `gh` + [gh-dash](https://github.com/dlvhdr/gh-dash), `yazi`,
`glow`, `bat`, `onefetch`, `starship`, `presenterm`.

```sh
brew install git neovim zellij ankitpokhrel/jira-cli/jira-cli fzf jq \
             lazygit gh yazi glow bat onefetch starship gettext
gh extension install dlvhdr/gh-dash
npm install -g @anthropic-ai/claude-code
```

On apt-based WSL, most of the above exist via `apt`/`brew` on Linux;
zellij and yazi are easiest from their GitHub release binaries or `cargo`.

## tk — command reference

| command | what it does |
|---|---|
| `tk` | fzf-pick one of your open Jira tickets, then open it |
| `tk FRD-123` | open (create if needed) worktree + zellij session for a ticket |
| `tk view [KEY] [--float]` | **the ticket TUI** (tk-tui) in this terminal; `--float` = zellij floating pane. KEY inferred from cwd → session name → picker. `tk tui` is an alias |
| `tk done [KEY]` | Claude-summarise the branch work → confirm → post as Jira comment + log learnings |
| `tk ls` | list ticket worktrees |
| `tk doctor` | check dependencies **and live Jira auth** — run this first when anything misbehaves |

**Inside the ticket TUI**: `j/k` scroll, `u/d` half-page, `gg`/`G`,
`J/K` select comment, **r** refresh, **c** new comment, **R** reply
(quotes the selected comment), **w** open in browser, `q` quit.
In compose: `ctrl-s` send, `esc` cancel (draft kept).
In the fzf pickers: `ctrl-j/k` move, `ctrl-d/u` half-page, `ctrl-f/b` page.

If Jira ever returns nothing ("No result found"), it's almost always an
expired API token — `tk doctor` will tell you and print the regeneration
steps. The token lives in `~/.config/jira-board/env` (single source of
truth; every tk entry point sources it).

## The daily loop

```sh
tk                # fzf over your open tickets → pick one
tk FRD-123        # or go straight there
```

`tk` creates the branch + worktree `Work/FRD-123-<slug>/`, seeds
`CLAUDE.local.md` so Claude spawns already knowing the ticket (it points
Claude at `jira issue view` — there is no local ticket file to drift),
installs the commit-prefix hook into the parent repo, and opens the zellij
session (or attaches, or — if you're already inside zellij — opens it as a
new tab):

```
┌───────────────────┬──────────────┐
│  nvim (62%)       │ ✳ claude     │  ← both columns stacked:
│                   │ ◫ ticket     │    expand what you need,
├───────────────────┤ ❯ shell      │    the rest fold to
│  ⎇ lazygit        │              │    title bars
└───────────────────┴──────────────┘
```

The `◫ ticket` pane is **tk-tui** (Rust/ratatui): title, description and
comments only — no metadata noise — with full vim motions and comment
replies (key table above). It's the same TUI you get anywhere via
`tk view`, so nothing about it is welded to this layout.
Pane movement is `Alt h/j/k/l`; `Ctrl h` is left free for nvim
(zellij's move mode lives on `Alt m`).

- Commits on the branch get auto-prefixed `FRD-123:` → Jira's GitHub
  integration links them to the ticket with zero effort.
- The claude pane runs `claude --continue`, which resumes the most recent
  conversation *in that directory* — so each worktree keeps its own thread.
- `r` in the ticket TUI re-pulls ticket text/comments. `tk ls` lists open worktrees.
- `Ctrl o w` (zellij session manager) is your ticket switcher; sessions
  survive reboots via session serialization.

When a chunk of work lands:

```sh
tk done           # inside the worktree
```

→ summarises `git log` + diffstat through `claude -p`, shows you the draft,
posts it as a **Jira comment** on confirmation, appends it to
`Knowledge/tickets/FRD-123.md`, and harvests any `#hdl`-tagged lines into
`Knowledge/hdl/inbox.md`. Weekly five-minute prune: promote inbox keepers
into `best-practices.md`. That's how HDL folklore becomes a document.

Papers:

```sh
paper https://arxiv.org/abs/2410.20672
```

→ templated note in `Knowledge/papers/` (+ PDF), opens in `$EDITOR`.
Everything — papers, ticket learnings, HDL practices — is one grep/telescope
surface.

## The scheme 🎨 — dopamine.nvim, everywhere

All colours live in **`.chezmoidata/palette.toml`** — edit → `chezmoi apply`
→ the zellij theme (tabs, ribbon, frames) re-renders. It's ported straight from
[dopamine.nvim](https://github.com/juliarozanova/dopamine.nvim)'s
`colors.lua`: the **dark** variant is active, with **mirage** and **light**
as commented blocks below it — swap a block, apply, done. Notable mapping:
session badge = copper accent, active tab = raspberry, and ANSI blue is
your slate keyword colour so blue-hungry TUIs (lazygit branches, fzf
pointers) stay in-vibe instead of shouting cyan.

**nvim** — keep installing the scheme through your plugin manager:

```lua
-- lazy.nvim
{ "juliarozanova/dopamine.nvim",
  lazy = false, priority = 1000,
  config = function() vim.cmd.colorscheme("dopamine") end },
-- lualine: require('lualine').setup({ options = { theme = 'dopamine' } })
```

`chezmoi apply` also drops a fresh reference copy of your `colors.lua` at
`~/.config/zellij/themes/dopamine-colors-reference.lua` so the exact hexes
are always one `:e` away when tweaking the palette. Your terminal emulator
scheme (Windows Terminal / WezTerm / kitty) can join via another
`.chezmoiexternal.toml` entry once you pick where that file lives.

## Anatomy

| path | what |
|---|---|
| `dot_local/bin/executable_tk` | the whole workflow: open / view / done / ls / doctor |
| `tk-tui/` | 🦀 the ratatui ticket TUI (`tk view`) — jira-cli `--raw` + ADF renderer |
| `run_onchange_before_20-build-tk-tui.sh.tmpl` | `cargo install`s tk-tui on `chezmoi apply` when its source changes |
| `dot_local/bin/executable_paper` | arXiv → knowledge note |
| `dot_local/share/tk/prepare-commit-msg` | ticket-prefix hook (installed per-repo by tk) |
| `dot_local/share/tk/summary-prompt.md` | the prompt `tk done` pipes into `claude -p` |
| `dot_config/zellij/templates/ticket.kdl.tpl.tmpl` | per-ticket layout (`$TICKET` baked by tk at open) |
| `dot_config/zellij/layouts/dash.kdl.tmpl` | home layout — plain `zellij` lands here |
| `.chezmoidata/palette.toml` | 🎨 the one file to retheme everything |

`tk-tui` colours come from the terminal's ANSI palette, so it inherits the
dopamine scheme (and any future light/dark switch) for free.

Optional zellij keybinding for a floating ticket pane from anywhere
(add inside the `keybinds { shared_except "locked" { … } }` block):

```kdl
bind "Alt y" { Run "tk" "view" "--float" { floating true; }; }
```
