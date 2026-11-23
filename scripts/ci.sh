#!/usr/bin/env bash
# FeatherAgent 本地 CI 门禁（与发布前检查等价）。
# 用法: scripts/ci.sh [--with-windows]
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:${PATH}"

CARGO_BIN="${CARGO:-cargo}"
"$CARGO_BIN" --version

scripts/version-check.sh
"$CARGO_BIN" fmt --check
"$CARGO_BIN" clippy --all-targets --locked -- -D warnings
"$CARGO_BIN" test --locked --all-targets
scripts/build-linux.sh

if [ "${1:-}" = "--with-windows" ]; then
  scripts/build-windows.sh
fi

echo "CI 通过: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
