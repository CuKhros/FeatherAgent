#!/usr/bin/env bash
# Linux x64 原生构建：与 build-windows.sh 共用版本门禁与 --locked。
# 用法: scripts/build-linux.sh
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="${HOME}/.cargo/bin:${PATH}"
scripts/version-check.sh
cargo build --release --bins --locked
echo "built:"
ls -lh target/release/featheragent target/release/featheragent-tui \
  target/release/featheragent-webui target/release/feather-plugin \
  target/release/feather-storage
