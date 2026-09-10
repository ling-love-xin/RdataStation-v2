# -*- coding: utf-8 -*-
import pathlib
for rel in [r"driver\registry\mod.rs", r"connection_manager.rs"]:
    p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src") / rel
    t = p.read_text(encoding="utf-8")
    n = t.count("```rust,ignore")
    t = t.replace("```rust,ignore", "```ignore")
    p.write_text(t, encoding="utf-8")
    print(rel, "replaced:", n)
