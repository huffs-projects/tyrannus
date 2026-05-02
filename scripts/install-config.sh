#!/usr/bin/env bash
# Creates ~/.config/tyrannus (respects XDG_CONFIG_HOME) and installs the default config if missing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${XDG_CONFIG_HOME:-$HOME/.config}/tyrannus"
DEFAULT_SRC="$ROOT/contrib/default-config.toml"

if [[ ! -f "$DEFAULT_SRC" ]]; then
  echo "tyrannus: missing $DEFAULT_SRC" >&2
  exit 1
fi

mkdir -p "$DEST"
if [[ ! -f "$DEST/config.toml" ]]; then
  cp "$DEFAULT_SRC" "$DEST/config.toml"
  echo "Installed default config at $DEST/config.toml"
else
  echo "Skipped: $DEST/config.toml already exists"
fi
