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

(or clone and run `./install.sh`.) `chezmoi apply` also auto-downloads
**zjstatus.wasm** — the status bar — via `.chezmoiexternal.toml`.

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
│                   │ ✳ claude     │  ← stacked: expand what
│  nvim (62%)       │ ⎇ lazygit    │    you need, the rest
│  TICKET.md        │ ◫ ticket     │    fold to title bars
│                   │ ❯ shell      │
└───────────────────┴──────────────┘
```

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

## Retheming 🎨

All colours live in **`.chezmoidata/palette.toml`**. Swap the hex values for
your scheme, run `chezmoi apply`, and the zellij theme + both status bars
re-render. (Shipped palette is a placeholder.)

## Old schemes (TODO — one edit)

`.chezmoiexternal.toml` has commented slots to pull your nvim config /
colour schemes straight from your previous dotfiles repo — fill in
`<user>/<old-repo>` and the paths, `chezmoi apply`, done. Your terminal
emulator scheme (Windows Terminal / WezTerm / Alacritty / kitty) can be
managed the same way once you decide which file it lives in.

## Anatomy

| path | what |
|---|---|
| `dot_local/bin/executable_tk` | the whole workflow: open / done / refresh / ls / doctor |
| `dot_local/bin/executable_paper` | arXiv → knowledge note |
| `dot_local/share/tk/prepare-commit-msg` | ticket-prefix hook (installed per-repo by tk) |
| `dot_local/share/tk/summary-prompt.md` | the prompt `tk done` pipes into `claude -p` |
| `dot_config/zellij/templates/ticket.kdl.tpl.tmpl` | per-ticket layout (palette baked by chezmoi, `$TICKET` by tk) |
| `dot_config/zellij/layouts/dash.kdl.tmpl` | home layout — plain `zellij` lands here |
| `.chezmoidata/palette.toml` | 🎨 the one file to retheme everything |
