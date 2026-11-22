import json
import os
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "featheragent.exe" if os.name == "nt" else "featheragent")
DEFAULT_ALLOW = "tool.fs.write,tool.shell"
ALLOW = os.environ.get("FEATHER_ALLOW", DEFAULT_ALLOW)


def run(session, text, timeout=180, allow=ALLOW):
    """跑一次 chat，返回 (events, 全部输出行, 耗时)"""
    t0 = time.time()
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
    ("t5-multi", "先读 workspace/s3-probe.txt，然后用一句话总结它的内容。", "read", "FEATHER"),
    ("t6-missing", "请读取 workspace/no-such-file-xyz.txt 并告诉我内容。", "read", "不存在"),
]

print(f"{'任务':12s} {'触发工具':22s} {'参数':28s} {'工具OK':7s} {'答复正确':8s} {'耗时':>6s}")
print("-" * 95)

stat = {"tool_expected": 0, "tool_fired": 0, "args_ok": 0, "result_ok": 0,
        "final_ok": 0, "no_tool_ok": 0, "no_tool_cases": 0}

for tid, prompt, exp_tool, exp_text in TASKS:
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
        stat["no_tool_cases"] += 1
        if not fired:
            stat["no_tool_ok"] += 1
    else:
        stat["tool_expected"] += 1
        if fired:
            stat["tool_fired"] += 1
        if fired and any(exp_tool in n for n in names):
            stat["args_ok"] += 1
        if res_ok:
            stat["result_ok"] += 1
        if final_ok:
            stat["final_ok"] += 1

    print(f"{tid:12s} {(','.join(names) if names else '(无)'):22s} {args_s:28s} "
          f"{('OK' if res_ok else ('ERR' if res_ok is False else '-')):7s} "
          f"{('OK' if final_ok else ('NO' if final_ok is False else '-')):8s} {dt:5.1f}s")

print("-" * 95)
print(f"应触发工具的任务数      : {stat['tool_expected']}")
print(f"  实际触发              : {stat['tool_fired']}  ({stat['tool_fired']}/{stat['tool_expected']})")
print(f"  工具名命中（未校验参数值）: {stat['args_ok']}  ({stat['args_ok']}/{stat['tool_expected']})")
print(f"  工具执行成功（严格口径）  : {stat['result_ok']}  ({stat['result_ok']}/{stat['tool_expected']})")
print(f"  最终答复正确          : {stat['final_ok']}  ({stat['final_ok']}/{stat['tool_expected']})")
print(f"不应调用工具的任务     : {stat['no_tool_cases']}，正确未调用 {stat['no_tool_ok']}")

# 检查写入是否真的落盘
w1 = os.path.join(ROOT, "workspace", "w1.txt")
print(f"\n写入校验 workspace/w1.txt : {'存在 -> ' + repr(open(w1, encoding='utf-8').read()) if os.path.exists(w1) else '❌ 不存在'}")
