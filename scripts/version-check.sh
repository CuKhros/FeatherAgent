#!/usr/bin/env bash
# 版本一致性门禁：Cargo.toml / Cargo.lock / 全部插件 manifest / WebUI 版本占位。
# Linux 与 Windows 打包脚本都先跑本脚本，避免双平台产物版本漂移。
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VERSION" ] || { echo "读不到 Cargo.toml version" >&2; exit 1; }

python3 - "$VERSION" <<'PY'
import json, pathlib, re, sys
version = sys.argv[1]
errors = []

lock = pathlib.Path("Cargo.lock").read_text(encoding="utf-8")
m = re.search(r'name = "featheragent"\nversion = "([^"]+)"', lock)
if not m:
    errors.append("Cargo.lock 缺少 featheragent 根包")
elif m.group(1) != version:
    errors.append(f"Cargo.lock 根包版本 {m.group(1)} != {version}")

for p in sorted(pathlib.Path("plugins").glob("*/plugin.json")):
    try:
        v = json.loads(p.read_text(encoding="utf-8"))["manifest"]["version"]
    except Exception as e:
        errors.append(f"{p}: 解析失败: {e}")
        continue
    if v != version:
        errors.append(f"{p}: manifest.version {v} != {version}")

web = pathlib.Path("web/index.html").read_text(encoding="utf-8")
if "v{{VERSION}}" not in web:
    errors.append("web/index.html 未使用 v{{VERSION}} 占位，WebUI 版本会漂移")

if errors:
    print("版本一致性检查失败：", file=sys.stderr)
    for e in errors:
        print(" -", e, file=sys.stderr)
    sys.exit(1)
print(f"版本一致性: {version}（Cargo/锁文件/{len(list(pathlib.Path('plugins').glob('*/plugin.json')))} 个插件/WebUI）")
PY
