#!/usr/bin/env bash
# dopamine-dotfiles bootstrap
#   ./install.sh                          → apply from this local clone
#   ./install.sh <user>/dopamine-dotfiles → apply straight from GitHub
set -euo pipefail

if ! command -v chezmoi >/dev/null 2>&1; then
  echo "installing chezmoi → ~/.local/bin"
  sh -c "$(curl -fsLS get.chezmoi.io)" -- -b "$HOME/.local/bin"
  export PATH="$HOME/.local/bin:$PATH"
fi

if [[ $# -ge 1 ]]; then
  chezmoi init --apply "$1"
else
  src="$(cd "$(dirname "$0")" && pwd)"
  chezmoi init --apply --source="$src"
fi

echo
echo "applied ✨  next:"
echo "  1. tk doctor            # see which tools still need installing"
echo "  2. jira init            # point jira-cli at your Jira (needs an API token)"
echo "  3. zellij               # lands on the dash layout"
