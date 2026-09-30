#!/usr/bin/env bash
# Executa verificacoes com ferramentas/caches locais, sem alterar o sistema.
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
export CARGO_HOME="$project_dir/.cache/rust/cargo"
export RUSTUP_HOME="$project_dir/.cache/rust/rustup"
export CARGO_TARGET_DIR="$project_dir/.cache/rust/target"
export TMPDIR="$project_dir/.cache/tmp"
export npm_config_cache="$project_dir/.cache/npm"
export PLAYWRIGHT_BROWSERS_PATH="$project_dir/.cache/playwright"
export PATH="$project_dir/.cache/node/bin:$CARGO_HOME/bin:$PATH"
mkdir -p "$TMPDIR"
exec "$@"
