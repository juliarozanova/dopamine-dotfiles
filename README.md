# dopamine-dotfiles ✨

One Jira ticket = one git worktree = one zellij session = one Claude context.

```
~/Dashboard/
├── Code/         main checkouts (clone your repos here)
├── Work/         per-ticket worktrees (tk manages these)
├── todo.md       work with no ticket of its own — `tk todo` edits it
└── Knowledge/
    ├── papers/   `paper <arxiv-url>` drops notes + PDFs here
    └── tickets/  per-ticket learnings, appended by tk done
```

## Install

One command on a new machine:

```sh
sh -c "$(curl -fsLS get.chezmoi.io)" -- init --apply <you>/dopamine-dotfiles
```

…or from a clone, `./install.sh`. Then:

```sh
tk doctor        # what's still missing, and whether Jira auth actually works
jira init        # jira-cli auth: choose "API token", paste a Jira API token
zellij           # lands on the dash
```

`chezmoi apply` is the whole install: it writes the configs, clones the
colorscheme to `~/.local/share/dopamine-light`, builds `tk-tui` with cargo, and
creates the `~/Dashboard` skeleton. It skips the build cleanly if cargo isn't
there yet, so you can install dependencies in any order and re-run.

Two secrets aren't in this repo and never should be:
`~/.config/jira-board/env` holding `export JIRA_API_TOKEN=…` (tk sources it on
every run), and whatever `jira init` writes to `~/.config/.jira/.config.yml`.

### Dependencies

Required: `git nvim zellij jira fzf jq lazygit claude curl gettext` (envsubst),
plus `cargo` — it builds `tk-tui` on `chezmoi apply`.
Delightful: `gh` + [gh-dash](https://github.com/dlvhdr/gh-dash), `yazi`,
`glow`, `bat`, `onefetch`, `starship`, `presenterm`.

**macOS**

```sh
brew install git neovim zellij ankitpokhrel/jira-cli/jira-cli fzf jq \
             lazygit gh yazi glow bat onefetch starship gettext rust
gh extension install dlvhdr/gh-dash
npm install -g @anthropic-ai/claude-code
```

**Ubuntu / WSL.** apt has the common ones; zellij, yazi and jira-cli are too
new for most releases, so take them from cargo or upstream binaries.

```sh
sudo apt update && sudo apt install -y \
     git neovim fzf jq curl gettext-base build-essential unzip

# rust (also builds tk-tui on chezmoi apply)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
cargo install zellij yazi-fm yazi-cli

# lazygit, gh, jira-cli: upstream releases
sudo snap install lazygit || cargo install --locked gitui   # either works
sudo apt install -y gh || sudo snap install gh
curl -sL "$(curl -s https://api.github.com/repos/ankitpokhrel/jira-cli/releases/latest \
  | jq -r '.assets[].browser_download_url | select(endswith("linux_x86_64.tar.gz"))')" \
  | tar xz -C /tmp && sudo install /tmp/jira_*/bin/jira /usr/local/bin/jira

gh extension install dlvhdr/gh-dash
npm install -g @anthropic-ai/claude-code   # needs node
```

`apt install neovim` is often old enough to upset LazyVim; if `tk doctor` is
happy but plugins misbehave, take neovim from its own PPA or an AppImage.

## Applying changes — the chezmoi loop

Bootstrapped with `chezmoi init`, the source lives at `~/.local/share/chezmoi`
and there's nothing to configure. If you'd rather edit a working tree directly,
point chezmoi at it once in `~/.config/chezmoi/chezmoi.toml`:

```toml
sourceDir = "/home/you/dopamine-dotfiles"    # /Users/you/… on macOS
```

…so **the repo you edit is the one `chezmoi apply` renders**. Without it chezmoi
keeps using its own clone and your edits quietly don't apply — with two copies
around, that's a confusing afternoon. `install.sh` passes the same thing via
`--source` when run from a clone.

```sh
chezmoi diff                                  # what would change, as a patch
chezmoi status                                # one line per differing file
chezmoi apply                                 # write everything
chezmoi apply ~/.config/wezterm/wezterm.lua   # …or one target
```

Nothing is live until you `apply` — editing `palette.toml` alone changes
nothing on disk.

If a target has changed since chezmoi last wrote it — including a file chezmoi
has never written, like a config you'd been hand-editing — `apply` stops and asks
rather than clobbering it. When you do mean to take the repo's version:
`chezmoi apply --force <target>`.

Two things happen on apply beyond writing files:
`run_onchange_before_20-build-tk-tui.sh.tmpl` re-runs `cargo install` whenever
tk-tui's sources change, and `.chezmoiexternal.toml` clones the dopamine
colorscheme to `~/.local/share/dopamine-light` (refreshed weekly) so nvim is
themed on a machine with no dev checkout.

## tk — command reference

| command | what it does |
|---|---|
| `tk` | fzf-pick one of your open Jira tickets, then open it |
| `tk FRD-123` | open (create if needed) worktree + zellij session for a ticket |
| `tk view [KEY] [--float]` | **the ticket TUI** (tk-tui) in this terminal; `--float` = zellij floating pane. KEY inferred from cwd → session name → picker. `tk tui` is an alias |
| `tk todo [--float]` | **the global checklist**: the `TODO` section of every open assigned ticket, plus `~/Dashboard/todo.md`, in one editable list. In a ticket session `Alt-t` toggles it; `--float` opens one anywhere else |
| `tk done [KEY]` | Claude-summarise the branch work → confirm → post as Jira comment + log learnings |
| `tk ls` | list ticket worktrees |
| `tk doctor` | check dependencies **and live Jira auth** — run this first when anything misbehaves |

**Inside the ticket TUI**: `j/k` scroll, `u/d` half-page, `gg`/`G`,
`J/K` select comment, **t** this ticket's TODO checklist, **r** refresh,
**c** new comment, **R** reply (quotes the selected comment), **w** open in
browser, **esc** back to the checklist if you came from it, `q` close.

**Inside the checklist** (`tk todo`, or `t` in the ticket TUI): `j/k` move,
`J/K` jump group, **space** tick, **i** edit text, **o** new item below the one
you're on, at its depth — and on a ticket with no checkboxes yet, which is how
you start one — **>>**/**<<** indent and outdent, **V** visual-line select (then `j`/`k` to
drag it and a single **>** or **<** to shift the lot), **dd** delete, **p** promote a local item onto a ticket,
**⏎** open the ticket you're on — the full ticket pane, from an item or from
the group heading — **esc** back to the list, **r** refresh, `q` close.

**Editing text** anywhere in tk is modal — the same small vim in the
checklist and the comment box: `hjkl w b e 0 ^ $ f t` motions, `d`/`c` plus a
motion, `x D C s`, `i a I A`, `u` undo. `⏎` (or `ZZ`) saves, `esc` from
normal mode discards. Comments send with **ZZ**, not `ctrl-s` — `ctrl-s` is
terminal flow control and never reaches the pane.
In the fzf pickers: `ctrl-j/k` move, `ctrl-d/u` half-page, `ctrl-f/b` page.

### Where the checklist gets its items

Two places, and only two:

- **In a ticket** — the checkboxes under a heading called `TODO` in the
  description. Jira's own action items; add them in the browser with `[]` at
  the start of a line, or press `o` in the checklist and tk writes the heading
  and the list for you if the ticket hasn't got one yet.
- **`~/Dashboard/todo.md`** — plain markdown `- [ ]` lines, for work with no
  ticket. Prose and headings around them are yours; only checkbox lines are
  ever rewritten, so the file stays something you'd happily open in nvim.

Items can be **nested**, to any depth. Jira's action items nest natively and
`todo.md` uses two spaces per level, so indentation is a first-class part of
both formats rather than something tk layers on top — indent something in the
browser and the pane shows it, press **>>** here and the ticket has it.
Outdenting brings an item's sub-items with it.

**Checkboxes outside the `TODO` heading are ignored on purpose** — acceptance
criteria, a checklist in the notes, whatever a colleague added. They still show
in the ticket pane, they just never reach the global list, so the list stays
yours.

```
description                        tk todo
  ¶ context…                       ─────────────────────────
  ## Acceptance criteria             JROZ-2  Get FraudGen…
    ☐ reviewed by two people   ✗       ☐ wire the eval harness
  ## TODO                              ☑ pin the dataset version
    ☐ wire the eval harness    ✓
    ☑ pin the dataset version  ✓
  ## Notes
    ☐ ask Sam re: latency      ✗
```

Tickets are scoped to jira-cli's configured project (`project.key` in
`~/.config/.jira/.config.yml`), which is what keeps Atlassian's "(Example)"
sample issues out of your list. `TK_TODO_JQL` overrides the query wholesale if
you want something else.

**Writing back is surgical.** tk never re-renders a description: it edits the
fetched document in place and puts the same document back, so tables, panels,
images, mentions and smart links — anything it has never heard of — come back
byte-identical. Every write re-reads first and refuses if the item moved on, so
editing the same ticket in a browser tab can't be silently overwritten. The one
lossy edit is rewording an item, which flattens formatting *inside that item*;
an item you open and close without typing is never written back.

If Jira ever returns nothing ("No result found"), it's almost always an
expired API token — `tk doctor` will tell you and print the regeneration
steps. The token lives in `~/.config/jira-board/env`, which tk sources on
every run so a refreshed token beats a stale `JIRA_API_TOKEN` inherited from a
long-lived zellij server. Keep your shell rc in step too, and
`zellij kill-all-sessions` afterwards so existing panes pick it up — `tk doctor`
spells out both.

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
replies (key table above). **`t`** swaps it for this ticket's checklist and
back; it's a mode rather than a second pane, because the description already
contains the TODO section and a second pane would just be showing a slice of
its neighbour. It's the same TUI you get anywhere via `tk view`, so nothing
about it is welded to this layout.
Pane movement is `Alt h/j/k/l`; `Ctrl h` is left free for nvim
(zellij's move mode lives on `Alt m`).

- Commits on the branch get auto-prefixed `FRD-123:` → Jira's GitHub
  integration links them to the ticket with zero effort.
- The claude pane runs `claude --continue`, which resumes the most recent
  conversation *in that directory* — so each worktree keeps its own thread.
- `r` in the ticket TUI re-pulls ticket text/comments. `t` swaps it for this
  ticket's checklist and back, keeping your place in the description.
- `Alt-t` brings the global checklist up over whatever you're doing, and
  `Alt-t` again puts it away. It's a floating pane the ticket layout declares
  and hides at session start, so the key only ever shows and hides it — it
  never opens a second copy, and stock `Alt-f` still governs the floating
  layer as usual. On the dash the checklist is already a pane in the right
  stack, so Alt-t has nothing to toggle there. `tk ls` lists open worktrees.
- `Ctrl o w` (zellij session manager) is your ticket switcher; sessions
  survive reboots via session serialization.

When a chunk of work lands:

```sh
tk done           # inside the worktree
```

→ summarises `git log` + diffstat through `claude -p`, shows you the draft,
posts it as a **Jira comment** on confirmation, and appends it to
`Knowledge/tickets/FRD-123.md`.

Papers:

```sh
paper https://arxiv.org/abs/2410.20672
```

→ templated note in `Knowledge/papers/` (+ PDF), opens in `$EDITOR`.
Papers and ticket learnings are one grep/telescope surface.

## The scheme 🎨 — one palette, everywhere

All colours live in **`.chezmoidata/palette.toml`** — edit → `chezmoi apply`
→ **everything below re-renders from it**. Nothing downstream holds its own
hexes:

| generated file | what reads it |
|---|---|
| `.config/nvim/lua/dopamine_palette.lua` | passed to the dopamine colorscheme as its `palette` option |
| `.config/wezterm/wezterm.lua` | `Dopamine Dark` / `Dopamine Light` schemes + ANSI slots |
| `.config/zellij/themes/dopamine.kdl` | ribbon, tabs, frames, tables, lists |
| `.config/tk/theme.json` | `tk view` / `tk todo` — the ratatui panes |
| `.config/gh-dash/config.yml` | the dash's `⇅ pull requests` pane |
| `.config/yazi/flavors/dopamine-{dark,light}.yazi/` | the dash's `🗀 files` pane, flavour + preview tmTheme |

Three variants ship as real tables — **`dark`** (everyday), **`light`**, and
**`mirage`** (ayu-flavoured). nvim carries all three; WezTerm ships dark and
light and follows macOS appearance. zellij can't ask the OS anything, so its
`dopamine` theme is an alias for whichever `[theme] variant` you select —
`dopamine-dark`/`-light`/`-mirage` are all emitted too, so `theme
"dopamine-light"` in `config.kdl` is an equivalent switch. Full rules in
[Switching dark ⇄ light](#switching-dark--light).

Role names are the colorscheme's own semantic slots (`accent`, `keyword`,
`panel_bg`, `vcs_added`…) rather than an ANSI-ish palette, because that's what
the highlight groups are written against. Notable mapping: session badge =
copper `accent`, active tab = raspberry `constant`, and ANSI blue is your slate
`keyword` so blue-hungry TUIs (lazygit branches, fzf pointers) stay in-vibe
instead of shouting cyan. The few ANSI slots with no semantic equivalent live in
palette.toml as `term_*` tints.

**Transparency.** WezTerm runs at `window_background_opacity = 0.8`, which only
blends cells carrying *no* explicit background — and it doesn't count "explicit
bg that happens to equal the scheme bg" as unset ([wezterm#1425][wz]). So a TUI
painting its own backdrop stays an opaque rectangle. Dropping
`text_background_opacity` would fix nvim but wash out every deliberate
background elsewhere (lazygit, btop, `ls`), so it stays at `1.0` and nvim opts
out on its side instead: the colorscheme's `transparent = true` clears the bg on
`Normal`/`NormalFloat`/`SignColumn`/`FoldColumn`/`WinSeparator`, leaving
`CursorLine`, `Pmenu` and `StatusLine` solid so highlights stay readable.

[wz]: https://github.com/wezterm/wezterm/issues/1425

**tk-tui + the dash.** `tk view` reads `~/.config/tk/theme.json` at startup and
maps palette roles onto the pane — `constant` for the ticket key, `special` for
status, `tag` for comment authors, `accent` for the compose border and caret.
Variant precedence is `$TK_THEME_VARIANT` → macOS appearance → `[theme] variant`
→ dark, so it follows light/dark like WezTerm and nvim do. Every field falls back
to a compiled-in dopamine-dark value, so `cargo install` on a bare machine still
looks right. Like nvim, it paints **no** backdrop, so the terminal's
transparency shows through — there's a test asserting no span ever sets a
background.

In the dash, `gh dash` takes its `theme.colors` from the palette (it has no
light/dark switching, so it follows `[theme] variant`). The `◷ my issues` pane
colours its section headers with an **ANSI index** rather than a baked-in hex —
WezTerm maps ANSI cyan to the palette's `tag`, so those headers follow the
terminal into light mode for free.

**yazi** ships both flavours (`dopamine-dark` / `dopamine-light`) and picks
between them from the terminal background on its own. Each is generated from one
shared body in `.chezmoitemplates/`, plus a `tmtheme.xml` whose scope→role
mapping mirrors the Neovim colorscheme — so a file previewed in yazi is coloured
the way the same file looks open in nvim.

A light variant can't just re-run the dark substitutions. Dark uses `bg` as the
*foreground* for text on its marker/mode chips (dark ink on bright chips); flip
to light and that becomes near-white on mid-tone pastels — every one of the 20
fg/bg pairs failed WCAG, the worst at 1.7:1. Hence the `on_*` roles in
palette.toml: the ink to use when a role is a *background*. Dark and mirage set
them to `bg` (so dark renders exactly as before), while light uses a near-black —
except on the terracotta `markup`, which is dark enough to want white instead.
The four remaining sub-3:1 pairs in light are accent-coloured text on the page,
the same trade dopamine-light already makes in nvim.

**nvim** — the scheme comes from its own repo, wired up in
`dot_config/nvim/lua/plugins/colorscheme.lua`:

```lua
-- a dev checkout if you have one, else the clone chezmoi vendors
local dev = vim.fn.expand("~/Dashboard/Code/dopamine-light")
local vendored = vim.fn.expand("~/.local/share/dopamine-light")
local scheme_dir = vim.fn.isdirectory(dev) == 1 and dev or vendored

{ "juliarozanova/dopamine-light", dir = scheme_dir,
  lazy = false, priority = 1000,
  config = function()
    local ok, palette = pcall(require, "dopamine_palette")
    require("dopamine").setup({ transparent = true, palette = ok and palette or nil })
  end },
-- lualine: require('lualine').setup({ options = { theme = 'dopamine' } })
```

Two fallbacks, and both matter on a fresh box: the `dir` prefers a dev checkout
so edits to the scheme show up on restart, but drops to the vendored clone when
`~/Dashboard/Code` doesn't exist — which is the normal case on WSL. And the
`pcall` means the plugin still works on its own built-in colours if
`dopamine_palette.lua` isn't there yet (nvim config used without chezmoi).

### Switching dark ⇄ light

Most of the stack follows **System Settings → Appearance** by itself. Flip the
macOS toggle and it changes live — no `chezmoi apply`, no restart:

| | how it notices |
|---|---|
| WezTerm | `window-config-reloaded` + `get_appearance()` |
| Neovim | checks `AppleInterfaceStyle` on focus + a 5s timer (`lua/config/autocmds.lua`) — **macOS only** |
| yazi | asks the terminal for its background colour |
| tk-tui | reads the appearance at startup — each `tk view` / `tk todo` is a fresh process |

Two can't ask the OS anything, so they follow `[theme] variant` in
`.chezmoidata/palette.toml`:

```toml
[theme]
variant = "light"     # dark | light | mirage
```

```sh
chezmoi apply
```

…then restart them: **zellij** reads its theme at session start, and **gh dash**
its config at launch. Zellij also gets `dopamine-dark` / `-light` / `-mirage`
emitted as named themes, so editing `theme "dopamine-light"` in `config.kdl` is
an equivalent switch.

So `variant` is *not* a global light switch — it moves those two and nothing
else. Setting it to `light` while macOS sits in dark mode is a legitimate
combination, not a mistake: zellij's chrome and the PR pane go light inside an
otherwise dark session. To move the whole stack, flip macOS **and** set
`variant` to match.

`mirage` is the odd one out: nvim has it (`:colorscheme dopamine-mirage`), and
zellij / gh-dash / tk-tui will render it via `variant`, but WezTerm only ships
dark and light — so the terminal underneath stays on whichever the OS says.

### Other machines, including WSL

`chezmoi apply` is the whole install. It vendors the colorscheme to
`~/.local/share/dopamine-light` via `.chezmoiexternal.toml`, so nvim is themed
on a box that has never heard of `~/Dashboard` — the lazy spec prefers a dev
checkout at `~/Dashboard/Code/dopamine-light` when one exists and falls back to
the vendored clone otherwise, so the same config serves both.

The only macOS-specific piece is *automatic* appearance-following:
`AppleInterfaceStyle` has no Linux equivalent. So off macOS:

- `lua/config/autocmds.lua` renders **without** a watcher — polling something
  that can't change is just a wasted process every few seconds — and nvim pins
  to `[theme] variant` through the LazyVim `colorscheme` opt.
- tk-tui's probe is `cfg!(target_os = "macos")`-gated, so it falls through to
  `variant` too, spawning nothing.
- WezTerm and yazi keep working unchanged: both ask the terminal/desktop, not
  the OS API.

Which means **off macOS, `variant` really is the global light switch** — set it,
`chezmoi apply`, restart. (On macOS it stays the two-mover described above.)

Nothing else assumes a platform: the tk-tui build script skips cleanly when
`cargo` is absent, tk-tui opens tickets through `jira open` rather than
`open(1)`, `tk` avoids GNU-only `find` flags, and
`macos_window_background_blur` is simply ignored off macOS.

**Four things WSL specifically needs**, none of them repo changes:

- **WezTerm's config lives on the Windows side.** WezTerm runs as a Windows
  app and reads `%USERPROFILE%\.wezterm.lua`, not the copy `chezmoi apply`
  writes inside WSL. Point Windows at the rendered file — a one-line
  `~/.wezterm.lua` on Windows doing
  `return dofile('\\\\wsl$\\Ubuntu\\home\\<you>\\.config\\wezterm\\wezterm.lua')` —
  or just use Windows Terminal and accept its own colours. Everything else
  (nvim, zellij, yazi, tk-tui) is themed inside WSL and unaffected.
- **`jira open` needs a browser bridge.** `sudo apt install wslu` gives
  `wslview`, which xdg-open then routes to your Windows browser. Without it
  `w` in the ticket pane does nothing.
- **Clipboard.** zellij's `copy_on_select` wants a clipboard command; install
  `wl-clipboard` (WSLg) or `xclip`, or set zellij's `copy_command` to
  `clip.exe`.
- **Glyphs.** The panes use `☑ ◫ ✳ ⎇ ⚑`; pick a Nerd Font in whichever
  terminal you're using or they render as boxes.

Keep the repo on the Linux filesystem (`~/dopamine-dotfiles`), not under
`/mnt/c`. Windows-mounted paths lose the executable bit, which matters for
everything in `dot_local/bin/`, and they are dramatically slower for git.

## Anatomy

| path | what |
|---|---|
| `dot_local/bin/executable_tk` | the whole workflow: open / view / todo / done / ls / doctor |
| `tk-tui/` | 🦀 the ratatui TUI (`tk view`, `tk todo`) — ticket pane + checklist, jira-cli `--raw` + ADF renderer, REST for descriptions |
| `run_onchange_before_20-build-tk-tui.sh.tmpl` | `cargo install`s tk-tui on `chezmoi apply` when its source changes |
| `dot_local/bin/executable_paper` | arXiv → knowledge note |
| `dot_local/share/tk/prepare-commit-msg` | ticket-prefix hook (installed per-repo by tk) |
| `dot_local/share/tk/summary-prompt.md` | the prompt `tk done` pipes into `claude -p` |
| `dot_config/zellij/templates/ticket.kdl.tpl.tmpl` | per-ticket layout (`$TICKET` baked by tk at open) |
| `dot_config/zellij/layouts/dash.kdl.tmpl` | home layout — plain `zellij` lands here; `☑ todo` is the expanded right pane |
| `dot_config/zellij/config.kdl` | keybinds — `Alt hjkl` movement, `Alt t` for the checklist |
| `run_once_before_10-dashboard.sh` | builds the `~/Dashboard` skeleton and seeds `todo.md` |
| `.chezmoidata/palette.toml` | 🎨 every colour, all three variants — the one file to edit |
| `dot_config/wezterm/wezterm.lua.tmpl` | terminal schemes + ANSI slots, generated from the palette |
| `dot_config/nvim/lua/dopamine_palette.lua.tmpl` | the palette as a Lua table, injected into the colorscheme |
| `dot_config/nvim/lua/config/autocmds.lua.tmpl` | macOS appearance watcher; renders empty off macOS |
| `.chezmoiexternal.toml` | vendors the colorscheme so a fresh machine needs no dev checkout |
| `dot_config/tk/theme.json.tmpl` | the palette as JSON, read by tk-tui at startup |
| `dot_config/gh-dash/config.yml.tmpl` | gh-dash settings + palette-driven `theme.colors` |
| `tk-tui/src/theme.rs` | role → colour mapping, with compiled-in dopamine-dark fallbacks |
| `dot_config/yazi/` | `theme.toml` + both flavour dirs (thin wrappers over the shared templates) |
| `.chezmoitemplates/yazi-flavor.toml` | shared yazi flavour body, rendered once per variant |
| `.chezmoitemplates/yazi-tmtheme.xml` | shared preview tmTheme, scopes mapped like the nvim scheme |

Want the same treatment for the ticket pane? Declare it in the ticket layout
next to `☑ todo` and bind a key to `ToggleFloatingPanes`, the way `Alt t`
works. Don't reach for `bind "Alt y" { Run "tk" "view"; }` — zellij's `Run`
opens a **new** pane every time it fires, so each press stacks another copy
until the floating layer is unusable. Declaring the pane once and toggling it
is the difference between a key you can lean on and one you can't.
