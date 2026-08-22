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

One thing happens on apply beyond writing files:
`run_onchange_before_20-build-tk-tui.sh.tmpl` re-runs `cargo install` whenever
tk-tui's sources change. (`.chezmoiexternal.toml` declares no externals — it's
just a commented-out recipe for vendoring the colorscheme, so nothing is
fetched.)

## tk — command reference

| command | what it does |
|---|---|
| `tk` | fzf-pick one of your open Jira tickets, then open it |
| `tk FRD-123` | open (create if needed) worktree + zellij session for a ticket |
| `tk view [KEY] [--float]` | **the ticket TUI** (tk-tui) in this terminal; `--float` = zellij floating pane. KEY inferred from cwd → session name → picker. `tk tui` is an alias |
| `tk todo [--float]` | **the global checklist**: the `TODO` section of every open assigned ticket, plus `~/Dashboard/todo.md`, in one editable list. `Alt-t` floats it from any session |
| `tk done [KEY]` | Claude-summarise the branch work → confirm → post as Jira comment + log learnings |
| `tk ls` | list ticket worktrees |
| `tk doctor` | check dependencies **and live Jira auth** — run this first when anything misbehaves |

**Inside the ticket TUI**: `j/k` scroll, `u/d` half-page, `gg`/`G`,
`J/K` select comment, **t** this ticket's TODO checklist, **r** refresh,
**c** new comment, **R** reply (quotes the selected comment), **w** open in
browser, `q` quit.

**Inside the checklist** (`tk todo`, or `t` in the ticket TUI): `j/k` move,
`J/K` jump group, **space** tick, **i** edit text, **o** new item,
**dd** delete, **p** promote a local item onto a ticket, **⏎** open the
ticket an item belongs to, **r** refresh, `q` quit.

**Editing text** anywhere in tk is modal — the same small vim in the
checklist and the comment box: `hjkl w b e 0 ^ $ f t` motions, `d`/`c` plus a
motion, `x D C s`, `i a I A`, `u` undo. `⏎` (or `ZZ`) saves, `esc` from
normal mode discards. Comments send with **ZZ**, not `ctrl-s` — `ctrl-s` is
terminal flow control and never reaches the pane.
In the fzf pickers: `ctrl-j/k` move, `ctrl-d/u` half-page, `ctrl-f/b` page.

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
replies (key table above). It's the same TUI you get anywhere via
`tk view`, so nothing about it is welded to this layout.
Pane movement is `Alt h/j/k/l`; `Ctrl h` is left free for nvim
(zellij's move mode lives on `Alt m`).

- Commits on the branch get auto-prefixed `FRD-123:` → Jira's GitHub
  integration links them to the ticket with zero effort.
- The claude pane runs `claude --continue`, which resumes the most recent
  conversation *in that directory* — so each worktree keeps its own thread.
- `r` in the ticket TUI re-pulls ticket text/comments. `t` swaps it for this
  ticket's checklist and back, keeping your place in the description.
- `Alt-t` floats the global checklist over whatever you're doing; `Alt-t`
  again dismisses it. `tk ls` lists open worktrees.
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

**nvim** — the scheme is a local checkout, wired up in
`dot_config/nvim/lua/plugins/colorscheme.lua`:

```lua
{ "juliarozanova/dopamine-light", dir = "~/Dashboard/Code/dopamine-light",
  lazy = false, priority = 1000,
  config = function()
    local ok, palette = pcall(require, "dopamine_palette")
    require("dopamine").setup({ transparent = true, palette = ok and palette or nil })
  end },
-- lualine: require('lualine').setup({ options = { theme = 'dopamine' } })
```

The `pcall` means the plugin still works standalone on its built-in colours if
`dopamine_palette.lua` isn't there yet (fresh machine, or nvim config used
without chezmoi).

### Switching dark ⇄ light

Most of the stack follows **System Settings → Appearance** by itself. Flip the
macOS toggle and it changes live — no `chezmoi apply`, no restart:

| | how it notices |
|---|---|
| WezTerm | `window-config-reloaded` + `get_appearance()` |
| Neovim | checks `AppleInterfaceStyle` on focus + a 5s timer (`lua/config/autocmds.lua`) — **macOS only** |
| yazi | asks the terminal for its background colour |
| tk-tui | reads the appearance at startup — each `tk view` is a fresh process |

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
`cargo` is absent, tk-tui opens tickets through `jira open` rather than `open(1)`,
and `macos_window_background_blur` is simply ignored off macOS.

## Anatomy

| path | what |
|---|---|
| `dot_local/bin/executable_tk` | the whole workflow: open / view / done / ls / doctor |
| `tk-tui/` | 🦀 the ratatui TUI (`tk view`, `tk todo`) — ticket pane + checklist, jira-cli `--raw` + ADF renderer, REST for descriptions |
| `dot_local/bin/executable_tk-todo-float` | what `Alt-t` runs: float the checklist, or toggle it away if it's already up |
| `run_onchange_before_20-build-tk-tui.sh.tmpl` | `cargo install`s tk-tui on `chezmoi apply` when its source changes |
| `dot_local/bin/executable_paper` | arXiv → knowledge note |
| `dot_local/share/tk/prepare-commit-msg` | ticket-prefix hook (installed per-repo by tk) |
| `dot_local/share/tk/summary-prompt.md` | the prompt `tk done` pipes into `claude -p` |
| `dot_config/zellij/templates/ticket.kdl.tpl.tmpl` | per-ticket layout (`$TICKET` baked by tk at open) |
| `dot_config/zellij/layouts/dash.kdl.tmpl` | home layout — plain `zellij` lands here |
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

Optional zellij keybinding for a floating ticket pane from anywhere
(add inside the `keybinds { shared_except "locked" { … } }` block):

```kdl
bind "Alt y" { Run "tk" "view" "--float" { floating true; }; }
```
