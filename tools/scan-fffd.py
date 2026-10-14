# -*- coding: utf-8 -*-
"""扫描整个仓库源码中的 U+FFFD 乱码（历史教训：chrome.rs 曾 648 处）。

用法：py tools/scan-fffd.py [仓库根]
注意：本机 PATH 里的 `python` 是 Windows Store 占位符（exit 49），请用 `py` 启动器。
"""
import os, sys

ROOT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else r'..')
SKIP = {'target', '.git', '.rds', 'node_modules', 'third_party', 'vendor'}

hits = []
for dirpath, dirnames, filenames in os.walk(ROOT):
    dirnames[:] = [d for d in dirnames if d not in SKIP and not d.startswith('.')]
    for fn in filenames:
        if not fn.endswith(('.rs', '.md', '.toml', '.py', '.sh', '.ps1', '.cmd', '.json')):
            continue
        p = os.path.join(dirpath, fn)
        try:
            with open(p, 'r', encoding='utf-8', errors='strict') as f:
                data = f.read()
        except (UnicodeDecodeError, OSError):
            continue
        cnt = data.count('\ufffd')
        if cnt:
            hits.append((os.path.relpath(p, ROOT), cnt))

for rel, cnt in sorted(hits):
    print(f"{cnt:4d}  {rel}")
print(f"\ntotal files with FFFD: {len(hits)}")
