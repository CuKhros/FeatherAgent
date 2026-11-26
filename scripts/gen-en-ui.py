import json, os, re, shutil, sys
# WebUI English is a separate, hand-maintained bundle (web/app.en.js + web/index.en.html);
# see scripts/check-en-web.py. Only Rust UI sources are generated here.
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / 'scripts' / 'en-ui'))
from tui_en import EN as TUI_EN
from kernel_en import EN as KERNEL_EN


def rust_lit(s):
    return json.dumps(s, ensure_ascii=False)[1:-1]

def zh_lits(src):
    vals = re.findall(r'"((?:[^"\\]|\\.)*)"', src)
    return sorted({x for x in vals if re.search('[\u4e00-\u9fff]', x)})

def gen_rust(src_path, out_path, ens, rename=None):
    src = src_path.read_text(encoding='utf-8')
    if '#[cfg(test)]' in src:
        src = src[:src.index('#[cfg(test)]')].rstrip() + '\n'
    zh = zh_lits(src)
    assert len(zh) == len(ens), (src_path, len(zh), len(ens))
    pairs = sorted(zip(zh, ens), key=lambda p: -len(p[0]))
    for z, e in pairs:
        src = src.replace('"' + z + '"', '"' + rust_lit(e) + '"')
    if rename:
        for a, b in rename:
            src = src.replace(a, b)
    left = [x for x in zh_lits(src)]
    assert not left, (src_path, left[:5])
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(
        '//! English UI variant generated from %s; do not edit directly.\n'
        '//! Regenerate: python3 scripts/gen-en-ui.py\n\n%s' % (src_path.name, src),
        encoding='utf-8',
    )
    print('generated', out_path)

def main():
    gen_rust(ROOT/'src/bin/tui.rs', ROOT/'src/bin/tui_en.rs', TUI_EN,
             rename=[('Usage: featheragent-tui', 'Usage: featheragent-tui-en'),
                     ('featheragent serve', 'featheragent-en serve')])
    gen_rust(ROOT/'src/bin/kernel.rs', ROOT/'src/bin/kernel_en.rs', KERNEL_EN,
             rename=[('Usage: featheragent <', 'Usage: featheragent-en <'),
                     ('featheragent serve', 'featheragent-en serve'),
                     ('"featheragent-webui.exe"', '"featheragent-webui-en.exe"'),
                     ('"featheragent-webui"', '"featheragent-webui-en"'),
                     ('launch featheragent-webui[.exe]', 'launch featheragent-webui-en[.exe]'),
                     ('featheragent-webui not found;', 'featheragent-webui-en not found;')])

if __name__ == '__main__':
    main()
    import subprocess
    cargo = os.environ.get('CARGO') or shutil.which('cargo') or str(Path.home() / '.cargo/bin/cargo')
    subprocess.run([cargo, 'fmt', '--all'], check=True)
