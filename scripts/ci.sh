#!/usr/bin/env bash
# FeatherAgent 本地 CI 门禁（与发布前检查等价）。
# 用法: scripts/ci.sh [--with-windows]
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:${PATH}"

CARGO_BIN="${CARGO:-cargo}"
"$CARGO_BIN" --version

scripts/version-check.sh
python3 scripts/gen-en-ui.py
git diff --exit-code -- src/bin/tui_en.rs src/bin/kernel_en.rs
python3 scripts/check-en-web.py
"$CARGO_BIN" fmt --check
"$CARGO_BIN" clippy --all-targets --locked -- -D warnings
"$CARGO_BIN" test --locked --all-targets
scripts/build-linux.sh
# N23 门禁：bench 必须整体可结束，不能卡到外部 150 秒超时。失败时保留 JSON 供诊断。
if ! timeout 90 target/release/featheragent bench --json > /tmp/fa-ci-bench.json; then
  echo "bench 失败或超时（90s），输出见 /tmp/fa-ci-bench.json" >&2
  cat /tmp/fa-ci-bench.json >&2 || true
  exit 1
fi
python3 - <<'PY_BENCH'
import json
with open('/tmp/fa-ci-bench.json', encoding='utf-8') as f:
    v = json.load(f)
if v.get('error'):
    raise SystemExit(f"bench 报错: {v['error']}")
gates = v.get('gates') or {}
required = ['binary_le_8mb', 'boot_lt_20ms', 'cold_spawn_lt_100ms',
            'write_p95_lt_100us', 'rss_le_15mb', 'rpc_p95_lt_1ms',
            'assemble_p95_lt_30ms']
bad = [k for k in required if gates.get(k) is not True]
if bad:
    raise SystemExit(f"bench 门禁未过: {bad}; gates={gates}")
# N34：gates 只允许布尔判定项，状态/原始读数在 diagnostics。
non_bool = [k for k, x in gates.items() if not isinstance(x, bool)]
if non_bool:
    raise SystemExit(f"gates 混入非布尔项: {non_bool}; gates={gates}")
diag = v.get('diagnostics') or {}
if diag.get('write_gate_status') not in ('pass', 'advisory'):
    raise SystemExit(f"write_gate_status 非法: {diag.get('write_gate_status')!r}; diagnostics={diag}")
if not isinstance(diag.get('write_gate_raw'), bool):
    raise SystemExit(f"write_gate_raw 缺失或类型错误; diagnostics={diag}")
if diag.get('platform') not in ('linux', 'windows'):
    raise SystemExit(f"diagnostics.platform 非法: {diag.get('platform')!r}")
m = v.get('metrics') or {}
print(f"bench 通过: boot_p50={m.get('boot_ms_p50')}ms cold_p50={m.get('cold_spawn_ms_p50')}ms")
PY_BENCH

if [ "${1:-}" = "--with-windows" ]; then
  scripts/build-windows.sh
fi

echo "CI 通过: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
