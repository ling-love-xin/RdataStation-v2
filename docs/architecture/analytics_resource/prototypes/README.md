# 资产库 / 分析存档（M6）· 分块原型

一个 UI 块一页，页内并排该块的所有状态。目的是让「改一块」不用同时读另外 21 个场景。

## 目录

```
prototypes/
├── index.html                    索引（先从这里开）
├── 01-panel-head.html            面板头            §2.1
├── 02-toolbar.html               工具栏            §2.2
├── 03-list-rows.html             列表行            §2.3
├── 04-group-fold.html            分组折叠          §2.4
├── 05-status-row.html            底部状态行        §2.5
├── 06-detail-file.html           File 型详情       §3.1
├── 07-detail-ref.html            引用型详情        §3.1 / §12.1.1
├── 08-detail-global.html         全局实例行详情    §13.2
├── 09-detail-insight.html        洞察集合卡        §12.2
├── 10-context-menu.html          右键菜单与拖拽    §3.2 / §2.4
├── 11-dialog-archive.html        归档              §4.1
├── 12-dialog-checkout.html       取回（检出）      §4.2
├── 13-dialog-versions.html       版本历史          §4.3
├── 14-dialog-diff.html           版本对比          §4.3 扩展
├── 15-dialog-trash.html          回收站            §4.4
├── 16-dialog-repair.html         索引修复          §4.5
├── 17-dialog-conflict.html       升级同名冲突      §13.3
├── 18-dialog-insight-archive.html 洞察归档         §12.2
├── 19-empty-states.html          空态与异常        §5
├── 20-dialog-rename.html         重命名 / 别名     §3.1 / §3.2（补页）
├── 21-dialog-group.html          新建 / 重命名分组  §2.4 / §3.2（补页）
├── 22-dialog-pick.html           从草稿箱归档选择  §2.1（补页）
└── _shared/
    ├── proto.css                 总集稿 <style> 的原样抽取（404 行，一行未改）
    ├── proto.js                  22 个图标 symbol 注入 + 明暗切换（IIFE，无依赖）
    └── page.css                  拆块页专属外壳 + 静态态显影 + 三个补页的最小样式（116 行）
```

## 改什么去哪

| 想改的 | 改这里 | 影响面 |
| --- | --- | --- |
| 间距 / 颜色 / 行高 / 徽标形状 / 弹层外观 | `_shared/proto.css` | 22 页同时生效 |
| 某一块的布局、文案、字段顺序 | 对应那一页的 HTML | 只影响这一页 |
| 某一块的某个新状态 | 在那页里加一个 `.variant` 小节 | 不影响别的块 |
| 拆块页的外壳（面包屑 / 标题条 / 变体标签 / 骨架示意） | `_shared/page.css` | 所有拆块页 |

`proto.css` 与总集稿的 `<style>` 是同源副本：**改视觉改这里**，不要去改总集稿（总集稿保留为整体走查的快照，两边不互相牵动）。

## 三种「静态显影」手法（页内并排的关键）

总集稿靠 `#wb` 上六个 `data-*` 状态位（`data-panel` / `data-status` / `data-detail` / `data-tabs` / `data-strip` / `data-dialog`）互斥显隐。拆块页不再造状态位，改用：

1. **原有开关类**：`proto.css` 里本来就带独立显影类的，直接加——`.headmenu.open` / `.pop.open` / `.alertbox.on` / `.toast.on` / `.undo.on` / `.ovbox.on` / `.row.sel` / `.row.miss` / `table.dt tr.on`。
2. **`.proto-on`**：只做 `display` 与定位还原，不动任何视觉——`.dlg.proto-on`（把 `position:absolute` + `translate(-50%,-50%)` 还原成静态流内）/ `.ctx.proto-on` / `.pstatus.proto-on` / `.strip.proto-on` / `.dstate.proto-on` / `.dp-insight.proto-on` / `.ctab.proto-on` / `.cbody.proto-on` / `.cb-none.proto-on` / `.st.proto-on` / `.alertbox.proto-on`。
3. **补的示意类**：总集稿没画、但规格要求或已实现的东西，只在 `page.css` 里补最小样式——`.ghead .dft`（默认分组标记）/ `.ghead.proto-drop`（拖拽落点）/ `.skrow` + `.skbar`（加载骨架）/ `.row.proto-hover` / `.row.proto-focus`；三个补页另补了 `.dlg-name`（384px）/ `.dlg-pick`（544px）/ `.picklist`（max 288px）/ `.pickrow`（勾选行）/ `.namehint`（单输入对话框的说明行）。

## 与另外两份原型的关系

| 文件 | 角色 | 本目录后 |
| --- | --- | --- |
| `../analytics-resource-prototype.html`（1962 行） | 总集稿：19 场景靠状态位互斥，用于**整体走查** | 未改动，继续用 |
| `../analytics-resource-prototype-design.md`（561 行） | 规格：§2 面板解剖 / §3 详情与右键 / §4 交互 / §5 状态矩阵 / §7 尺寸 / §13 双实例 | 每页 `<p-spec>` 与页脚都引它的章节号 |
| 本目录 | 按块切开，每页可单独评审、单独迭代 | 新增 |

每页结构固定：页头（面包屑 + 块名 + §徽标 + 规格说明 + 上下块导航）→ 若干 `.variant`（A / B / C…）→ 页脚「本块样式 + 落地代码」。页脚的落地代码就是评审结论可以直接指到的文件。

## 状态

- 23 个页面（22 块 + 索引）全部落盘，并通过结构校验：**0 未定义类名 / 0 缺失图标 symbol / 0 死链**，两份 CSS 括号平衡（`proto.css` 351/351、`page.css` 90/90）。校验脚本为只读扫描（页面里的 `class` 逐个对回两份 CSS 的 309 个类名、`#i-*` 对回 `proto.js` 的 22 个 symbol、`href/src` 对回真实文件）。
- 已用 Chrome headless 实截 11 页并逐张目视：`index` / `03` / `04` / `06` / `11` / `16` / `17` / `19`，以及本轮补的 `20` / `21` / `22`；并与总集稿默认态对照，**240px 行的名字截断与尾部裁切行为一致**。其余 11 页共用同一外壳与样式，未逐页截图。
- 本轮补的三页（`20` / `21` / `22`）来源是「代码里有、评审页里没有」的三条入口：详情点头部改名、分组区新建分组、面板头 `＋ ▾` 从草稿箱归档。页内状态都对应实现里真实存在的分支（空名 / 值没变 / 超 `MAX_DRAFT_ROWS` 截断）。
- 未做：① 页内没有交互脚本（除 `proto.js` 的图标与明暗切换）——它们是**静态评审页**，不是可点击原型；② 四个动作的时序图仍在设计稿 §4；③ 应用级外壳（标题栏 / 活动栏 / 应用状态栏）不属本模块，未做页；④ 总集稿末尾六张说明卡片未拆成独立页，结论已折进各页规格行；⑤ **§13 双实例链路**（分发 / 实例化 / 跨项目引用四处）仍未成页——设计稿 §12.2.1 与 §13 那几段自己标着「待用户决策」，画页等于替用户先拍板。
