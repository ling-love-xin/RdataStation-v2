# -*- coding: utf-8 -*-
import pathlib, re
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\shared\src\macros.rs")
t = p.read_text(encoding="utf-8")
# 把裸 ``` 的宏示例块标为 ignore（宏使用需完整错误类型上下文，真实用法由单元测试覆盖）
n = 0
def repl(m):
    global n
    n += 1
    return "```ignore\n" + m.group(1) + "```"
new, c = re.subn(r"```\n([\s\S]*?)```", repl, t)
p.write_text(new, encoding="utf-8")
print("macros doc blocks tagged:", c)
