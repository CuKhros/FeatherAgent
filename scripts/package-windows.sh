#!/usr/bin/env bash
# Windows x64 单包（用户解压即测）：5 个 exe + plugins + web + docs。
# 用法: scripts/package-windows.sh [--skip-build]
# 产物: dist/FeatherAgent-<version>-windows-x64.zip + .sha256 + manifest.json
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
ARCH=windows-x64
TARGET=x86_64-pc-windows-gnu
BIN=target/$TARGET/release
DIST=dist
NAME=FeatherAgent-$VERSION-$ARCH
STAGE=$DIST/.stage-$NAME

[ "$SKIP_BUILD" -eq 1 ] || scripts/build-windows.sh

for e in featheragent featheragent-tui featheragent-webui feather-plugin feather-storage; do
  [ -f "$BIN/$e.exe" ] || { echo "缺少 $BIN/$e.exe，先跑 scripts/build-windows.sh" >&2; exit 1; }
done

rm -rf "$STAGE"
mkdir -p "$STAGE/$NAME"
cp "$BIN"/*.exe "$STAGE/$NAME/"
cp -r plugins "$STAGE/$NAME/plugins"
cp -r web "$STAGE/$NAME/web"
mkdir -p "$STAGE/$NAME/tools"
cp tools/feather_client.py "$STAGE/$NAME/tools/" 2>/dev/null || true
mkdir -p "$STAGE/$NAME/docs"
cp docs/*.md "$STAGE/$NAME/docs/" 2>/dev/null || true
cp README.md "$STAGE/$NAME/"
cp docs/Windows测试说明.md "$STAGE/$NAME/START-HERE.md"

# 体积门禁：内核 exe ≤ 8 MiB
CORE_BYTES=$(stat -c %s "$STAGE/$NAME/featheragent.exe")
LIMIT=$((8 * 1024 * 1024))
[ "$CORE_BYTES" -le "$LIMIT" ] || { echo "FAIL: featheragent.exe $CORE_BYTES > 8 MiB" >&2; exit 1; }

# 依赖检查：不能出现 mingw 运行时 DLL
if command -v x86_64-w64-mingw32-objdump >/dev/null; then
  for e in "$STAGE/$NAME"/*.exe; do
    if x86_64-w64-mingw32-objdump -p "$e" | grep -qiE 'DLL Name: (libgcc|libwinpthread|libstdc++)'; then
      echo "FAIL: $(basename "$e") 依赖 mingw 运行时 DLL" >&2
      exit 1
    fi
  done
fi

mkdir -p "$DIST"
rm -f "$DIST/$NAME.zip" "$DIST/$NAME.zip.sha256"
( cd "$STAGE" && zip -r -9 -q "$NAME.zip" "$NAME" )
mv "$STAGE/$NAME.zip" "$DIST/$NAME.zip"
rm -rf "$STAGE"
SUM=$(sha256sum "$DIST/$NAME.zip" | cut -d' ' -f1)
echo "$SUM  $NAME.zip" > "$DIST/$NAME.zip.sha256"

python3 - "$DIST/$NAME.zip" "$VERSION" "$ARCH" "$CORE_BYTES" <<'PY'
import hashlib, json, os, sys, zipfile
zpath, version, arch, core_bytes = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
files = []
with zipfile.ZipFile(zpath) as z:
    for n in z.namelist():
        if n.endswith('/'):
            continue
        data = z.read(n)
        files.append({"path": n, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
manifest = {
    "version": version,
    "arch": arch,
    "target": "x86_64-pc-windows-gnu",
    "package": os.path.basename(zpath),
    "package_bytes": os.path.getsize(zpath),
    "package_sha256": hashlib.sha256(open(zpath, 'rb').read()).hexdigest(),
    "core_bin_bytes": core_bytes,
    "files": files,
}
out = os.path.join(os.path.dirname(zpath), "FeatherAgent-%s-%s.manifest.json" % (version, arch))
json.dump(manifest, open(out, 'w', encoding='utf-8'), ensure_ascii=False, indent=2)
print("manifest:", out)
PY

echo "$NAME: $(du -h "$DIST/$NAME.zip" | cut -f1) zip, kernel exe $CORE_BYTES bytes"
echo "sha256: $SUM"
