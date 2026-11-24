import json
import os
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "featheragent.exe" if os.name == "nt" else "featheragent")
DEFAULT_ALLOW = "tool.fs.write,tool.shell"
ALLOW = os.environ.get("FEATHER_ALLOW", DEFAULT_ALLOW)
# v1.1.8 起 workspace_root 是项目容器，默认项目是 workspace/default。
# 如需测其他项目，设置 FEATHER_PROJECT_DIR 指向该项目目录。
WORKSPACE = os.environ.get(
    "FEATHER_PROJECT_DIR", os.path.join(ROOT, "workspace", "default")
)
os.makedirs(WORKSPACE, exist_ok=True)
PROBE = os.path.join(WORKSPACE, "s3-probe.txt")
if not os.path.exists(PROBE):
    with open(PROBE, "w", encoding="utf-8") as f:
        f.write("FEATHER_S3_OK_42")
RUN_ID = str(int(time.time() * 1000))


def run(session, text, timeout=180, allow=ALLOW):
    """跑一次 chat，返回 (events, 全部输出行, 耗时)。每次用独立 session，避免历史累积污染。"""
    t0 = time.time()
    session = f"{session}-{RUN_ID}"
    args = [EXE, "chat", "--root", ROOT, "--session", session, "--text", text]
    if allow:
        args += ["--allow", allow]
    p = subprocess.run(
        args,
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout,
    )
    dt = time.time() - t0
    evs = []
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            o = json.loads(line)
        except Exception:
            continue
        if o.get("method") == "event":
            pr = o.get("params", {})
            evs.append((pr.get("kind"), pr.get("payload", {})))
    return evs, p.stdout, dt


def summarize(evs):
    tools, calls, results, errors = [], [], [], []
    final, finish = "", None
    for k, pl in evs:
        if k == "tool.call":
            calls.append(pl)
        elif k == "tool.result":
            results.append(pl)
        elif k == "tool.error":
            errors.append(pl)
        elif k == "tool.failed":
            errors.append(pl)
        elif k == "model.message":
            final = pl.get("text", "") if pl.get("text") else final
            finish = pl.get("finish")
    return calls, results, errors, final, finish


TASKS = [
    # (id, 提示词, 期望工具(或 None=不应调用), 期望最终文本包含)
    ("t1-read", "请用工具读取 workspace/s3-probe.txt，把文件内容原样告诉我。", "read", "FEATHER_S3_OK_42"),
    ("t2-write", "请用工具把这段文本写入 workspace/w1.txt：HELLO_W_99", "write", None),
    ("t3-shell", "请用 shell 工具执行命令 echo S3SHELL 并把输出告诉我。", "shell", "S3SHELL"),
    ("t4-notool", "1 加 1 等于几？直接回答。", None, "2"),
    # t5/t6 显式写出“必须调用工具/只调用一次”：不写死这条约束时，4B 模型会直接凭上下文猜答案，
    # 或失败后换路径反复重试，S3 口径无法稳定复现（rc.2.1 真机报告 H4）。
    ("t5-multi", "必须先调用 tool.fs.read 工具读取 workspace/s3-probe.txt，再用一句话总结它的内容。禁止不调用工具直接回答。", "read", "FEATHER"),
    ("t6-missing", "请调用 tool.fs.read 读取 workspace/no-such-file-xyz.txt 并把结果告诉我。只调用一次读取工具；如果读取失败，直接说明失败原因，不要重试、不要换路径、不要改用其他工具。", "read", "不存在", True),
]

print(f"项目工作区: {WORKSPACE}")
print(f"{'任务':12s} {'触发工具':22s} {'参数':28s} {'工具执行':10s} {'答复正确':8s} {'耗时':>6s}")
print("-" * 95)

stat = {"tool_expected": 0, "tool_fired": 0, "args_ok": 0, "result_ok": 0,
        "expected_cases": 0, "expected_error": 0, "expected_error_ok": 0, "real_failed": 0,
        "final_ok": 0, "no_tool_ok": 0, "no_tool_cases": 0}

for tid, prompt, exp_tool, exp_text, *flags in TASKS:
    expect_error = bool(flags and flags[0])
    if expect_error:
        stat["expected_cases"] += 1
    try:
        evs, out, dt = run(tid, prompt)
    except subprocess.TimeoutExpired:
        print(f"{tid:12s} TIMEOUT")
        continue
    calls, results, errors, final, finish = summarize(evs)
    names = [str(c.get("name") or c.get("tool") or "?") for c in calls]
    args = [c.get("arguments") or c.get("args") for c in calls]
    args_s = json.dumps(args[0], ensure_ascii=False) if args else "-"
    if len(args_s) > 26:
        args_s = args_s[:26] + "…"

    fired = len(calls) > 0
    # 严格口径：有 tool.result、未被拒绝、result 内无 error，且没有 tool.error/tool.failed。
    if errors:
        res_ok = False
    elif results:
        res_ok = all(
            isinstance(r, dict)
            and not r.get("denied")
            and not r.get("error")
            and not (isinstance(r.get("result"), dict) and r["result"].get("error"))
            for r in results
        )
    else:
        res_ok = None
    final_ok = (exp_text in (final or "")) if exp_text else None

    if exp_tool is None:
        # 不应调用工具：只看是否被误触发，不参与工具执行成功率。
        status = "未调用" if not fired else "ERR"
        stat["no_tool_cases"] += 1
        if not fired:
            stat["no_tool_ok"] += 1
    else:
        # 三档口径：成功 / 预期业务错误且模型正确报告 / 真失败。
        if res_ok:
            status = "OK"
        elif expect_error and errors and final_ok:
            status = "预期失败"
        else:
            status = "ERR"
        stat["tool_expected"] += 1
        if fired:
            stat["tool_fired"] += 1
        if fired and any(exp_tool in n for n in names):
            stat["args_ok"] += 1
        if status == "OK":
            stat["result_ok"] += 1
        elif status == "预期失败":
            stat["expected_error"] += 1
            stat["expected_error_ok"] += 1
        else:
            stat["real_failed"] += 1
        if final_ok:
            stat["final_ok"] += 1

    print(f"{tid:12s} {(','.join(names) if names else '(无)'):22s} {args_s:28s} "
          f"{status:10s} "
          f"{('OK' if final_ok else ('NO' if final_ok is False else '-')):8s} {dt:5.1f}s")

print("-" * 95)
print(f"应触发工具的任务数      : {stat['tool_expected']}")
print(f"  实际触发              : {stat['tool_fired']}  ({stat['tool_fired']}/{stat['tool_expected']})")
print(f"  工具名命中（未校验参数值）: {stat['args_ok']}  ({stat['args_ok']}/{stat['tool_expected']})")
print(f"  工具执行成功（严格口径）  : {stat['result_ok']}  ({stat['result_ok']}/{stat['tool_expected']})")
print(f"  预期业务错误且答复正确     : {stat['expected_error_ok']}  (预期负样本 {stat['expected_cases']} 个)")
print(f"  真失败                    : {stat['real_failed']}  ({stat['real_failed']}/{stat['tool_expected']})")
print(f"  最终答复正确          : {stat['final_ok']}  ({stat['final_ok']}/{stat['tool_expected']})")
print(f"不应调用工具的任务     : {stat['no_tool_cases']}，正确未调用 {stat['no_tool_ok']}")

# 检查写入是否真的落盘
w1 = os.path.join(WORKSPACE, "w1.txt")
print(f"\n写入校验 workspace/w1.txt : {'存在 -> ' + repr(open(w1, encoding='utf-8').read()) if os.path.exists(w1) else '❌ 不存在'}")
