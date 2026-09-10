# -*- coding: utf-8 -*-
"""清除全部 dbg 打点（view.rs + panels.rs），保持 R30 形态。"""
import pathlib

for path, marker in [
    (r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs', '[dbg] WorkbenchView::render'),
    (r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs', '[dbg] EditorPanel::render'),
]:
    p = pathlib.Path(path)
    t = p.read_text(encoding='utf-8')
    lines = t.splitlines(keepends=True)
    out = []
    removed = 0
    for line in lines:
        if marker in line or line.strip().startswith('eprintln!("[dbg]'):
            removed += 1
            continue
        out.append(line)
    p.write_text(''.join(out), encoding='utf-8')
    print(f'{path}: removed {removed} lines')
