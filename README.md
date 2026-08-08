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

Required: `git nvim zellij jira fzf jq lazygit claude gettext` (envsubst).
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

These are **all** the commands that exist today. (`tk tui` is *not* one of
them — a Rust/ratatui ticket pane is in development in `tk-tui/`, see
"Work in progress" below.)

| command | what it does |
|---|---|
| `tk` | fzf-pick one of your open Jira tickets, then open it |
| `tk FRD-123` | open (create if needed) worktree + zellij session for a ticket |
| `tk done [KEY]` | Claude-summarise the branch work → confirm → post as Jira comment + log learnings |
| `tk refresh` | re-pull ticket text + comments into `TICKET.md` (run inside a worktree) |
| `tk ls` | list ticket worktrees |
| `tk doctor` | check dependencies **and live Jira auth** — run this first when anything misbehaves |

Inside the ticket pane (`◫ ticket` in the layout): **r** refresh,
**c** one-line comment, **w** open in browser. In the fzf pickers:
`ctrl-j/k` move, `ctrl-d/u` half-page, `ctrl-f/b` page.

If Jira ever returns nothing ("No result found"), it's almost always an
expired API token — `tk doctor` will tell you and print the regeneration
steps. The token lives in `~/.config/jira-board/env` (single source of
truth; every tk entry point sources it).

## The daily loop

```sh
tk                # fzf over your open tickets → pick one
tk FRD-123        # or go straight there
```

`tk` creates the branch + worktree `Work/FRD-123-<slug>/`, writes the ticket
text + comments into `TICKET.md`, seeds `CLAUDE.local.md` so Claude spawns
already knowing the ticket, installs the commit-prefix hook into the parent
repo, and opens the zellij session (or attaches, or — if you're already
inside zellij — opens it as a new tab):

```
┌───────────────────┬──────────────┐
│  nvim (62%)       │ ✳ claude     │  ← both columns stacked:
│  TICKET.md        │ ◫ ticket     │    expand what you need,
├───────────────────┤ ❯ shell      │    the rest fold to
│  ⎇ lazygit        │              │    title bars
└───────────────────┴──────────────┘
```

The ticket pane is interactive (`tk-ticket-pane`): it shows just the ticket
body and comments, with **[r]** refresh (also re-syncs `TICKET.md`),
**[c]** add a one-line Jira comment, **[w]** open in browser.
Pane movement is `Alt h/j/k/l`; `Ctrl h` is left free for nvim
(zellij's move mode lives on `Alt m`).

- Commits on the branch get auto-prefixed `FRD-123:` → Jira's GitHub
  integration links them to the ticket with zero effort.
- The claude pane runs `claude --continue`, which resumes the most recent
  conversation *in that directory* — so each worktree keeps its own thread.
- `tk refresh` re-pulls ticket text/comments. `tk ls` lists open worktrees.
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
| `dot_local/bin/executable_tk` | the whole workflow: open / done / refresh / ls / doctor |
| `dot_local/bin/executable_tk-ticket-pane` | interactive ticket pane: [r]efresh / [c]omment / [w]eb |
| `dot_local/bin/executable_paper` | arXiv → knowledge note |
| `dot_local/share/tk/prepare-commit-msg` | ticket-prefix hook (installed per-repo by tk) |
| `dot_local/share/tk/summary-prompt.md` | the prompt `tk done` pipes into `claude -p` |
| `dot_config/zellij/templates/ticket.kdl.tpl.tmpl` | per-ticket layout (palette baked by chezmoi, `$TICKET` by tk) |
| `dot_config/zellij/layouts/dash.kdl.tmpl` | home layout — plain `zellij` lands here |
| `.chezmoidata/palette.toml` | 🎨 the one file to retheme everything |
| `tk-tui/` | 🚧 work in progress — see below |

## Work in progress: `tk-tui` (Rust/ratatui ticket pane)

`tk-tui/` holds a half-built ratatui rewrite of the ticket pane. **It does
not compile or run yet** — only the data-source (`jira.rs`, shells out to
jira-cli `--raw`) and ADF renderer (`adf.rs`) exist; `main.rs`/`app.rs`/`ui.rs`
are still to come. Until it lands, the ticket pane is the bash
`tk-ticket-pane`, and there is no `tk tui`/`tk view` subcommand.

Planned (per `to_fix.md` + plan): full vim motions (hjkl, u/d, gg/G),
comment selection + quoted **reply**, `tk view [KEY] [--float]` to run it in
any pane or float it over a session, TICKET.md removal (nvim opens plain),
and a chezmoi `run_onchange` hook that `cargo install`s it on apply.
