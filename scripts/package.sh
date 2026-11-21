#!/usr/bin/env bash
# 三产物打包（设计文档 §8）：core / gui / plugins。
# 用法: scripts/package.sh [--skip-build]
# 产物: dist/featheragent-<product>-<version>-linux-x64.tar.gz + MANIFEST + sha256
set -euo pipefail
cd "$(dirname "$0")/.."

SKIP_BUILD=0
for a in "$@"; do
  case "$a" in
    --skip-build) SKIP_BUILD=1 ;;
    *) echo "未知参数: $a" >&2; exit 2 ;;
  esac
done

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VERSION" ] || { echo "读不到 Cargo.toml version" >&2; exit 1; }
ARCH=linux-x64
DIST=dist
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

if [ "$SKIP_BUILD" -eq 0 ]; then
  cargo build --release --bins
fi
BIN=target/release

[ -x "$BIN/featheragent" ] || { echo "缺少 $BIN/featheragent，先 cargo build --release" >&2; exit 1; }

# ---------- core ----------
CORE="$STAGE/featheragent-core"
mkdir -p "$CORE/plugins"
cp "$BIN/featheragent" "$BIN/feather-plugin" "$BIN/feather-storage" "$BIN/featheragent-tui" "$CORE/"
cp -r plugins/* "$CORE/plugins/"
cp README.md "$CORE/"
mkdir -p "$CORE/docs"
cp docs/*.md "$CORE/docs/" 2>/dev/null || true

# ---------- gui ----------
GUI="$STAGE/featheragent-gui"
mkdir -p "$GUI/web"
cp "$BIN/featheragent-gui" "$GUI/"
cp web/index.html web/app.js web/style.css "$GUI/web/"
cp README.md "$GUI/"
mkdir -p "$GUI/docs" && cp docs/*.md "$GUI/docs/" 2>/dev/null || true

# ---------- plugins ----------
PLUG="$STAGE/featheragent-plugins"
mkdir -p "$PLUG/plugins"
cp -r plugins/* "$PLUG/plugins/"
cp README.md "$PLUG/"

# 体积门禁：内核二进制 ≤ 8 MiB（§2.3；未测不写数字，超了直接失败）
CORE_BIN_BYTES=$(stat -c %s "$CORE/featheragent")
CORE_LIMIT=$((8 * 1024 * 1024))
if [ "$CORE_BIN_BYTES" -gt "$CORE_LIMIT" ]; then
  echo "FAIL: featheragent 内核 $CORE_BIN_BYTES bytes > 8 MiB" >&2
  exit 1
fi

mkdir -p "$DIST"
rm -f "$DIST"/featheragent-*-"$VERSION"-"$ARCH".tar.gz "$DIST"/featheragent-*-"$VERSION"-"$ARCH".tar.gz.sha256

manifest="$DIST/featheragent-manifest-$VERSION-$ARCH.json"
{
  echo "{"
  echo "  \"version\": \"$VERSION\","
  echo "  \"arch\": \"$ARCH\","
  echo "  \"core_bin_bytes\": $CORE_BIN_BYTES,"
  echo "  \"products\": ["
  first=1
  for name in featheragent-core featheragent-gui featheragent-plugins; do
    d="$STAGE/$name"
    sz=$(du -sb "$d" | cut -f1)
    tar -C "$STAGE" -czf "$DIST/$name-$VERSION-$ARCH.tar.gz" "$name"
    sum=$(sha256sum "$DIST/$name-$VERSION-$ARCH.tar.gz" | cut -d' ' -f1)
    echo "$sum  $name-$VERSION-$ARCH.tar.gz" > "$DIST/$name-$VERSION-$ARCH.tar.gz.sha256"
    [ "$first" -eq 1 ] || echo ","
    first=0
    printf '    {"name":"%s","bytes":%s,"sha256":"%s","tarball":"%s"}' \
      "$name" "$sz" "$sum" "$name-$VERSION-$ARCH.tar.gz"
  done
  echo ""
  echo "  ]"
  echo "}"
} > "$manifest"

for name in featheragent-core featheragent-gui featheragent-plugins; do
  echo "$name: $(du -sh "$STAGE/$name" | cut -f1) unpacked, $(stat -c %s "$DIST/$name-$VERSION-$ARCH.tar.gz") bytes tar.gz"
done
echo "kernel bin: $CORE_BIN_BYTES bytes (limit $CORE_LIMIT)"
echo "manifest: $manifest"
