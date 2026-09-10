# -*- coding: utf-8 -*-
import pathlib

FENCE = "```"
for rel in [r"crates\project\src\lib.rs", r"crates\project\src\store.rs"]:
    p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2") / rel
    t = p.read_text(encoding="utf-8")
    lines = t.splitlines(keepends=True)
    out = []
    removed = 0
    for line in lines:
        stripped = line.strip()
        # 只处理 doc 注释里的围栏行（//! ```）
        if line.startswith("//!") and FENCE in line:
            removed += 1
            continue
        out.append(line)
    p.write_text("".join(out), encoding="utf-8")
    print(rel, "fences removed:", removed)
