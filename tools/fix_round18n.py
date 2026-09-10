# -*- coding: utf-8 -*-
import pathlib, re

base = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")
targets = [
    "driver\\router.rs",
    "driver\\smart_pool.rs",
    "driver\\mod.rs",
    "driver\\standard_pool.rs",
    "driver\\wasm\\mod.rs",
    "driver\\jdbc\\mod.rs",
    "driver\\registry\\mod.rs",
    "connection_manager.rs",
]
CHARS = set("│├└═▼─┌┐└┘┬┴┼")
changed = 0
for rel in targets:
    p = base / rel
    t = p.read_text(encoding="utf-8")
    # 在注释块中找 ``` 或 ```xxx 开闭对；只处理 ``` 后没有语言的
    # 简单策略：正则匹配 "```\n...```"（首尾都是裸 ```）
    def repl(m):
        global changed
        inner = m.group(1)
        # 仅当块含图表字符且不含 Rust 关键字样式时标记 text
        if any(c in CHARS for c in inner):
            changed += 1
            return "```text\n" + inner + "```"
        return m.group(0)

    new, n = re.subn(r"```\n([\s\S]*?)```", repl, t)
    if n:
        p.write_text(new, encoding="utf-8")
        print(rel, "blocks tagged:", n)
print("total tagged:", changed)
