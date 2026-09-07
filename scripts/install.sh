#!/usr/bin/env bash
# #7/install — non-cargo fallback installer.
#
# Builds the release binary and copies it onto PATH without requiring
# `cargo install` rights. Idempotent: safe to re-run after every `git pull`.
#
# Install location: $BB_INSTALL_DIR (default: $HOME/.local/bin).
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

install_dir="${BB_INSTALL_DIR:-$HOME/.local/bin}"

cargo build --release --locked
install -Dm755 target/release/bb "$install_dir/bb"

echo "Installed: $install_dir/bb"

case ":$PATH:" in
    *":$install_dir:"*)
        echo "PATH: ok ($install_dir is on PATH)"
        ;;
    *)
        echo "PATH: warning — $install_dir is not on your PATH."
        echo "  Add it, e.g.: export PATH=\"$install_dir:\$PATH\""
        ;;
esac
