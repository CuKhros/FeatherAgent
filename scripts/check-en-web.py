"""校验 WebUI 英文包与中文包的功能骨架一致。

WebUI 的英文不是由模板替换生成，而是独立维护的 `web/app.en.js` /
`web/index.en.html`。本脚本只做结构校验，避免新增功能只落在中文包：

1. 英文 JS 的字符串字面量里不能残留中文；
2. 中英 JS 的函数名、元素 id、RPC 方法集合必须一致；
3. 中英 HTML 的元素 id 必须一致，英文页必须引用 `app.en.js` 且 `lang="en"`。
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CJK = re.compile("[\u4e00-\u9fff]")


def literals(src: str) -> list[str]:
    out: list[str] = []
    for pat in (
        r'"((?:[^"\\]|\\.)*)"',
        r"'((?:[^'\\]|\\.)*)'",
        r"`((?:[^`\\]|\\.)*)`",
    ):
        out += re.findall(pat, src, re.S)
    return out


def check_js(zh_path: Path, en_path: Path) -> list[str]:
    problems: list[str] = []
    zh = zh_path.read_text(encoding="utf-8")
    en = en_path.read_text(encoding="utf-8")
    zh_lit = [x for x in literals(en) if CJK.search(x)]
    if zh_lit:
        problems.append(f"{en_path.name} 仍含中文字面量: {zh_lit[:3]}")

    for label, pat in (
        ("函数", r"function\s+(\w+)"),
        ("元素 id", r'id="([^"]+)"'),
        ("RPC 方法", r'rpc\("([^"]+)"'),
    ):
        a = set(re.findall(pat, zh))
        b = set(re.findall(pat, en))
        if a != b:
            problems.append(
                f"{en_path.name} 与中文包的{label}集合不一致: 缺 {sorted(a - b)}，多 {sorted(b - a)}"
            )
    return problems


def check_html(zh_path: Path, en_path: Path) -> list[str]:
    problems: list[str] = []
    zh = zh_path.read_text(encoding="utf-8")
    en = en_path.read_text(encoding="utf-8")
    if 'lang="en"' not in en:
        problems.append(f"{en_path.name} 缺少 lang=en")
    if "web/app.en.js" not in en and "/app.js" not in en:
        problems.append(f"{en_path.name} 没有引用 app 脚本")
    if any(CJK.search(line) and "<!--" not in line for line in en.splitlines()):
        problems.append(f"{en_path.name} 仍含中文正文")
    a = set(re.findall(r'id="([^"]+)"', zh))
    b = set(re.findall(r'id="([^"]+)"', en))
    if a != b:
        problems.append(
            f"{en_path.name} 与中文页的元素 id 不一致: 缺 {sorted(a - b)}，多 {sorted(b - a)}"
        )
    return problems


def main() -> int:
    problems = check_js(ROOT / "web/app.js", ROOT / "web/app.en.js")
    problems += check_html(ROOT / "web/index.html", ROOT / "web/index.en.html")
    if problems:
        for p in problems:
            print("FAIL:", p, file=sys.stderr)
        return 1
    print("WebUI English bundle OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
