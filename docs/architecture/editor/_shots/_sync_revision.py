# -*- coding: utf-8 -*-
import io
p = r'D:\RdataStation\RDS\RdataStation-v2\docs\architecture\editor\editor-prototype-design.md'
s = io.open(p, encoding='utf-8').read()
anchor = '状态栏=面板专属：左=连接 ▸ 会话 · 内核 · 单元数（含 stale 计数）· 变量数 · 脏，右=英文微标 + 行｜列编辑切换。'
add = ('\n> **2026-10-11 修订（模式切换移至工具栏）**：⑯ 模式切换位置**从标题栏移至工具栏最左侧**（通用 / SQL / 分析三原型同步），'
       '样式由挖空槽改为**普通工具栏项**（图标 + 模式名 + ▾；工具栏底色即 `sidebar`，挖空无凹陷对比，故不再用 `title_bar.slot`）；'
       '在 SQL 模式中模式指示器为**固定项**，不参与 `···` 工具栏自定义排序（`tbOrder`/`moveToolbar`/`moveGroup` 均排除 `modeSlot`）。')
assert anchor in s, 'anchor not found'
s = s.replace(anchor, anchor + add, 1)
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('done ⑯ count =', s.count('⑯'))
