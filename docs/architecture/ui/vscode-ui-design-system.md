# VS Code UI 设计系统参考（面向 RdataStation-v2 复用）

> 服务对象：RdataStation-v2（Tauri 2.0 + Rust + gpui-kit 组件库，桌面数据库工具）的 UI 治理。本文件是
>
> **面向复用**
>
> 的 VS Code UI 设计系统资料：布局、交互、组件、主题，细致到尺寸 / 间距 / 字号 / 颜色 token / 交互状态 / 行为约定。
> **调研基线**
>
> ：
>
> `microsoft/vscode`
>
>  仓库 
>
> `main`
>
>  分支（2026-10-11 抓取，经 jsdelivr CDN / GitHub API）；版本演进另标注版本号（1.28 / 1.83 / 1.141 等）。
> **来源档位**
>
> （每个数值后标注，全文件通用）：
> 【O】= 官方文档（URL 见文末来源清单）
> 【S】= 源码实证（microsoft/vscode 或 vscode-codicons 仓库内文件路径，尽量带行号）
> 【C】= 社区整理（URL + 整理者）
> 【未确认】= 本次未能实证，不凭记忆填充
> **颜色取值口径**
>
> ：主题默认值分两层 —— 主题 JSON 覆盖层（
>
> `*_modern.json`
>
>  / 
>
> `*_vs.json`
>
>  / 
>
> `*_plus.json`
>
> ，include 链式继承）与源码 
>
> `registerColor()`
>
>  默认层（
>
> `src/vs/platform/theme/common/colors/*.ts`
>
> ）。本文件 "暗色" 列以当前出厂暗色主题 
>
> **Dark Modern**
>
>  生效值为准，括号内给源码层经典默认（Dark+ 观感）；亮色以 
>
> **Light Modern**
>
>  为准。注意 2026 版新增 
>
> `2026-dark.json`
>
>  处于主题链最上层（
>
> `2026-dark → dark_modern → dark_plus → dark_vs`
>
> ），个别值有差异（如 badge.background 
>
> `#307E9F`
>
> 、button.background 
>
> `#297AA0`
>
> ），本文件在 Dark Modern 值后对差异显著的场景给出 2026 值。
> **一个重要勘误**
>
> ：任务线索中的 
>
> `dark_defaults.json`
>
>  / 
>
> `light_defaults.json`
>
>  在当前 main 分支
>
> **已不存在**
>
> （jsdelivr 两次 404 + 
>
> `extensions/theme-defaults/package.json`
>
>  的 10 个主题清单佐证【S】）。workbench UI 色的权威出处是主题 JSON + 
>
> `registerColor()`
>
>  源码默认层，不要再用旧记忆中的 
>
> `dark_defaults`
>
>  路径。



***

## 0. 一页速查（布局 × 组件 × 关键尺寸 × 来源）



| 区域 / 组件              | 关键尺寸                                                                                                     | 来源档位 |
| -------------------- | -------------------------------------------------------------------------------------------------------- | ---- |
| 工作台根字号               | `.monaco-workbench` = **13px**，行高 1.4em；body = 11px                                                      | S    |
| 标题栏（Win/Linux）       | 高 **30px**（无命令中心）/ **35px**（有命令中心或窗口控制）                                                                  | S    |
| 窗口控制按钮（Win/Linux）    | 每按钮 **46×46px**、图标 16px、三按钮合计 138px；macOS 预留 70px                                                        | S    |
| 命令中心（Command Center） | 高 **22px**、宽 38vw（max **600px**）、圆角 cornerRadius-medium                                                  | S    |
| 菜单栏                  | 在标题栏左区；按钮最小宽 36px、行高 30px；compact 态进活动栏（高 35px）                                                          | S    |
| 活动栏                  | 宽 **48px**（`--activity-bar-width` 默认）、item 高 **48px**、图标 **24px**（`--activity-bar-icon-size` 默认）、激活条 2px | S    |
| 活动栏徽标                | 高 16px、圆角 20px、字号 9px、top 24px right 8px；compact：13px                                                    | S    |
| 侧边栏                  | 最小宽 **170px**；头部（视图切换行）35px；分区头 22px                                                                     | S    |
| 状态栏                  | 高 **22px**、字号 12px、item 内距 0 5px、item 间距 3px                                                             | S    |
| 面板                   | 头部 35px；默认底部；对齐方式 center/justify/left/right                                                              | S+O  |
| 分隔条 Sash             | 默认 **4px**、hover 延迟 300ms、双击复位                                                                           | S    |
| 编辑器 Tab              | 高 **35px**（default）/ **22px**（compact）；Modern UI 32/28；字号 13px；内距左 10px（收缩带图标 5px）                       | S+O  |
| Tab 宽度               | fit（默认）120px 起 /shrink min 80px /fixed 50–160px                                                          | S    |
| 面包屑                  | 行高随 Tab 高（35/22）；分隔符 `/` 0.9em；picker 行 22px                                                             | S    |
| 编辑器滚动条               | 垂直 **14px**、水平 **12px**（Monaco 内部）                                                                       | S    |
| Minimap              | gutter 8px、maxColumn 120                                                                                 | S    |
| 行号                   | 预留 5 位；gutter 修改 / 新增 / 删除 蓝 / 绿 / 红                                                                     | S    |
| 编辑器字号                | **14px**（Win/Linux）/ **12px**（macOS）；行高自动 ≈19px / 18px                                                   | S    |
| 树 / 列表行高             | **22px**（`treeView.ts:1246` ITEM\_HEIGHT=22）                                                             | S    |
| 树箭头槽                 | 宽 16px、图标 10px；缩进参考线 1px 竖线                                                                              | S    |
| 输入框                  | 内距 4px 6px、圆角 4px、高≈24px（内容撑开）                                                                           | S    |
| 按钮                   | 内距 4px 8px、圆角 4px、字号 12px / 行高 16px、高≈26px                                                               | S    |
| **徽标 Badge**         | **最小 18×18px、内距 3px 5px、圆角 11px 胶囊、字号 11px**；`.long` 变体圆角 2px                                            | S    |
| 工作台滚动条               | 槽 / 滑块 **10px**；淡入 100ms / 淡出 800ms                                                                      | S    |
| Quick Input（命令面板）    | 宽 **600px**、行高 22px、列表 max **440px**（20 行）                                                               | S    |
| 通知 Toast             | 最大宽 **450px**；右下角 right 3px /bottom 25px；条目内距 10px 5px                                                   | S    |
| 对话框                  | 最小宽 **480px**、max 90vw/90vh、padding 8px、主消息 14px/600                                                     | S    |
| Codicon              | 默认渲染 **16px**（字体 `16px/1`）；活动栏放大至 24px                                                                   | S    |



***

## 1. 布局五段框架总览

VS Code 工作台（Workbench）由五个区段构成（【O】userinterface /custom-layout 文档 +【S】DOM 结构）：



```
┌────────────────────────────────────────────────────────────┐
│ 标题栏行（Title Bar 区，含窗口控制 / 命令中心 / 菜单栏）      │ ← 30–35px
├──────────┬──────────────────────────────┬──────────────────┤
│ 活动栏    │ 编辑器区（Editor Region）     │ 辅助侧边栏         │
│ 48px      │  ├ 编辑器组标题行（Tab/面包屑）│ (Secondary Side)  │
│ (默认左侧) │  ├ 编辑器组（可分割网格）      │                  │
│          │  └ 状态行                     │                  │
├──────────┴──────────────────────────────┴──────────────────┤
│ 面板区（Panel Region，默认底部，对齐方式可调）                 │ ← 头 35px
├────────────────────────────────────────────────────────────┤
│ 状态栏（Status Bar，22px，底部全宽）                        │ ← 22px
└────────────────────────────────────────────────────────────┘
```

要点（【O】[https://code.visualstudio.com/docs/configure/custom-layout](https://code.visualstudio.com/docs/configure/custom-layout) ）：



* **主侧边栏（Primary Side Bar）**：默认在左，可整体移到右（`workbench.sideBar.location: right` 或命令）。

* **辅助侧边栏（Secondary Side Bar）**：默认右侧，可隐藏（`workbench.secondarySideBar.defaultVisibility`）。

* **活动栏位置**：`Default / Top / Bottom / Hidden`；置顶 / 置底时 Account 与 Manage 按钮移到标题栏右侧。

* **活动栏尺寸**：`workbench.activityBar.compact: true` 切紧凑（复古样式）；仅侧边位置可用。

* **面板位置与对齐**：`View > Appearance > Panel Position`（左 / 右 / 底 / 顶）；对齐 `Center`（默认，只跨编辑器区宽）/ `Justify`（全窗宽）/ `Left` / `Right`；`Maximize Panel Size` 展开占满编辑器区。

* **命令面板位置**：Quick Input 可拖拽移动（抓顶部边缘）或用预设位置。

* **通知位置**：`workbench.notifications.position`（默认 bottom-right；可 bottom-left /top-right）。

* **Pinned tabs / Locked editor groups**：固定 tab、锁定编辑器组，保持关键视图常驻。

* **浮动窗口 Compact Mode**：浮动编辑器窗口可去除多余 UI（Set Compact Mode）。

DOM 层级（编辑器区，【S】`editorgroupview.css`）：`.part.editor > .content > .grid-view-container > .editor-group-container > .title(.tabs-and-actions-container + .breadcrumbs-control) + .editor-container(.editor-instance)`。

**根字号与字体栈**（【S】`src/vs/workbench/browser/media/style.css`）：



* `body { font-size: 11px }`；`.monaco-workbench { font-size: 13px; line-height: 1.4em }` —— **UI 根字号 13px**。

* Windows：`"Segoe WPC", "Segoe UI", sans-serif`（简中追加 `"Microsoft YaHei"`）；macOS：`-apple-system, BlinkMacSystemFont`（简中 `"PingFang SC"`）；Linux：`system-ui, "Ubuntu", "Droid Sans"`。

* 等宽（编辑器）：Windows `Consolas, 'Courier New', monospace`；macOS `'SF Mono', Monaco, Menlo`。

* 缩放：`window.zoomLevel` 按 20% 整档作用于根（`--window-zoom-factor`）。



***

## 2. 窗口 Chrome 组件

### 2.1 标题栏 Title Bar



| 项                 | 值                                                                                                                                                        | 来源                                                                      |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| 高度（Windows/Linux） | 命令中心可见或窗口控制启用时 **35px**（`DEFAULT_CUSTOM_TITLEBAR_HEIGHT = 35; // includes space for command center`）；否则 **30px**                                         | S `src/vs/platform/window/common/window.ts` + `titlebarPart.ts:227-231` |
| macOS             | 默认**原生标题栏**（红绿灯按钮）；自定义标题栏可开（`window.titleBarStyle: custom`）                                                                                              | O v1\_28 与 userinterface                                                |
| 窗口控制按钮（Win/Linux） | 每按钮宽 **46px**、高 100%、图标 **16px**（`::before` 16×16、行高 16px）；三按钮合计 `calc(138px / zoom)`；macOS 预留 **70px**                                                  | S `titlebarpart.css`                                                    |
| 窗口标题              | `font-size: 12px`；居中对齐（Win 在左 / 居中因布局而异）                                                                                                                 | S                                                                       |
| 应用图标（Appicon）     | 宽 **35px**、logo 背景 16px                                                                                                                                  | S                                                                       |
| 命令中心              | 高 **22px**、宽 `min(38vw, 600px)`、圆角 `--vscode-cornerRadius-medium`、1px 边框（`--vscode-commandCenter-border`）                                                | S                                                                       |
| 菜单栏               | 在标题栏左区（`.titlebar-left > .menubar`），最小宽 **36px**                                                                                                         | S `titlebarpart.css:221-224` + `titlebarPart.ts:392`                    |
| 非激活窗口             | `.titlebar.inactive` 整体 `opacity: 0.6`                                                                                                                   | S                                                                       |
| 窗口控制 hover        | 普通按钮 `rgba(255,255,255,0.1)`；**关闭按钮 hover&#x20;**`rgba(232,17,35,0.9)`（红）                                                                                | S                                                                       |
| 拖拽区               | 标题栏空白为窗口拖拽区（菜单按钮区在拖拽区之上）                                                                                                                                 | S                                                                       |
| 颜色 token          | `titleBar.activeBackground` `#181818` / `activeForeground` `#CCCCCC`（Dark Modern）；`inactiveBackground` / `inactiveForeground` 同系降一档；`titleBar.border` 可配 | S/M                                                                     |

> 复用提示：RdataStation 标题栏 36px（2.25rem）与 VS Code 的 35px（命令中心态）几乎同值；若做命令中心 / Quick Open 入口，可参照 22px 高 + 38vw (max 600px) 宽 + 圆角 token 的做法（RdataStation SKILL 中 Quick Open 入口为 20rem×1.625rem=320×26px，偏窄偏矮，见 §8）。

### 2.2 菜单栏 Menu Bar



| 项                    | 值                                                                                                                                                                                              | 来源                       |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| 结构                   | Windows/Linux 下默认内嵌在标题栏左区同一行（`.titlebar-left`）；`window.menuBarVisibility` 四态：`visible / classic / hidden / compact`                                                                            | S `titlebarPart.ts`      |
| 按钮尺寸                 | 菜单按钮最小宽 **36px**；`menubar-menu-title` 行高 **30px**（与标题栏 30px 行高匹配）                                                                                                                              | S `titlebarpart.css:271` |
| compact 态            | 菜单收进活动栏溢出（汉堡 /`...`），活动栏内 `.menubar.compact` 高 **35px**                                                                                                                                        | S `activityaction.css`   |
| 弹层（下拉菜单）             | 复用 `.monaco-menu`；`menu.background` `#1F1F1F` / `menu.selectionBackground` `#3994BC26`（2026-dark）/ 经典 `#0078D4`（dark\_vs）；弹层圆角 / 阴影用 `--vscode-cornerRadius-large`、`--vscode-shadow-lg`（下拉组件层） | S/M                      |
| hover / focus / open | hover：`outline: dashed 1px var(--vscode-menubar-selectionBorder)`；focus / open：`outline: solid 1px`；底色 `--vscode-menubar-selectionBackground`（`#3994BC26`），前景 `menubar-selectionForeground`    | S `menubarControl.css`   |
| 单个菜单项行高 / 内距         | **未确认**（`.monaco-menu-item` 的 CSS 本轮未取到；社区观感约 24px 行高，不写死）                                                                                                                                     | 未确认                      |
| 键盘                   | Alt 聚焦菜单栏；↑↓ 选择、←→ 切换菜单、Enter 打开、Esc 关闭                                                                                                                                                        | O userinterface          |

### 2.3 活动栏 Activity Bar



| 项         | 值                                                                                                                                                                                                                                                                                                                                                              | 来源                      |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| 宽度        | **48px**：`width: var(--activity-bar-width, 48px)` —— **core 布局 CSS 变量，默认 48px**                                                                                                                                                                                                                                                                                | S `activitybarpart.css` |
| item（单项）  | `height: var(--activity-bar-action-height, 48px)`、宽同活动栏 48px                                                                                                                                                                                                                                                                                                   | S `activityaction.css`  |
| 图标        | `font-size: var(--activity-bar-icon-size, 24px)` —— **默认 24px**（社区流传 22px 偏小；源码默认 24px）                                                                                                                                                                                                                                                                        | S                       |
| 激活指示条     | 左侧 2px 竖条（`.active-item-indicator`，色 `activityBar.activeBorder` / `activityBar.activeBackground`）                                                                                                                                                                                                                                                              | S                       |
| 徽标        | `.badge .badge-content`：高 **16px**、行高 16px、字号 **9px**、圆角 **20px**、位置 top **24px** / right **8px**；**compact 活动栏**：高 13px、字号 8px、top 13px /right 6px                                                                                                                                                                                                            | S `activityaction.css`  |
| 顶部 / 底部按钮 | 默认底部 Account / Manage；活动栏置顶 / 置底时移到标题栏右侧                                                                                                                                                                                                                                                                                                                       | O custom-layout         |
| 颜色 token  | `activityBar.background` `#181818`（DM）/ `#333333`（Dark+）；`activityBar.foreground` `#D7D7D7`；`activityBar.inactiveForeground`（未激活项，暗色比 foreground 降一档，2026 主题有覆盖）；`activityBarBadge.background` `#0078D4`（DM）/ `#007ACC`（Dark+）、`activityBarBadge.foreground` `#FFFFFF`；`activityWarningBadge.background` `#CCA700`、`activityErrorBadge.background` `#f48771` 等 | S/M                     |
| tooltip   | hover 显示工具提示（ActivityBarHover，延迟约数百 ms；具体常量未逐项实证）                                                                                                                                                                                                                                                                                                              | 未确认（行为 O userinterface） |
| 位置        | 默认随主侧边栏在外缘；可 Top/Bottom/Hidden（见 §1）                                                                                                                                                                                                                                                                                                                           | O                       |

> 与 RdataStation 对照：宽度两边都是 48px，可直接对齐；item 高 VS Code 48px（正方）vs RdataStation 44×40（扁），图标 24 vs 28px，见 §8。

### 2.4 侧边栏 Side Bar



| 项                         | 值                                                                                                                                                                                                                 | 来源                     |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| 最小宽度                      | **170px**（`minimumWidth: 170`）                                                                                                                                                                                    | S `sidebarPart.ts`     |
| 默认宽度                      | 由视图最优宽决定（≥最小宽，常用 300px 起跳）；无固定默认像素                                                                                                                                                                                | S（preferredWidth 逻辑）   |
| 头部（视图切换行）                 | 高 **35px**（`.part > .title` 通用：`height: 35px; padding: 0 8px`；标题 label `padding-left: 12px`；h2 字号 **11px**）                                                                                                       | S `part.css`           |
| 分区头（如 EXPLORER / OUTLINE） | 高 **22px**（`.viewlet .collapsible.header` 相关）                                                                                                                                                                     | S `sidebarpart.css:49` |
| 视图工具栏                     | action-item 16px 图标、`active-item-indicator` 2px                                                                                                                                                                   | S                      |
| 颜色 token                  | `sideBar.background` `#181818`（DM）/ `#252526`（Dark+）；`sideBar.foreground` `#CCCCCC`；`sideBar.border`（可配 1px 分隔）；`sideBarTitle.foreground`；`sideBarSectionHeader.background` `#181818` / `.foreground` / `.border` | S/M                    |

### 2.5 状态栏 Status Bar



| 项            | 值                                                                                                                                                                                                                      | 来源                    |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| 高度           | **22px**                                                                                                                                                                                                               | S `statusbarpart.css` |
| 字号           | **12px**                                                                                                                                                                                                               | S                     |
| item 内距 / 间距 | label `padding: 0 5px`；item 之间 `margin: 3px`；整行 `padding: 0 10px`（左右）                                                                                                                                                  | S                     |
| 行高           | `line-height: 22px`                                                                                                                                                                                                    | S                     |
| hover        | `background: var(--vscode-statusBarItem-hoverBackground)`；`outline: 1px solid var(--vscode-contrastActiveBorder)`（HC）                                                                                                  | S                     |
| active（按压）   | `--vscode-statusBarItem-activeBackground`                                                                                                                                                                              | S                     |
| 语义型          | `.statusbar-item.prominent`（`statusBarItem.prominentBackground`）；`.warning` / `.error`（`statusBarItem.warningBackground` / `errorBackground`，黄 / 红）；`.remote`（`statusBarItem.remoteBackground` `#16825D` 绿）；`.offline` | S                     |
| 特殊状态         | 调试会话：`statusBar.debuggingBackground` `#CC6633`（Dark+ 经典）；无文件夹：`statusBar.noFolderBackground`                                                                                                                           | S/M                   |
| 颜色 token     | `statusBar.background` `#181818`（DM）/ `#007ACC`（Dark+ 蓝！）；`statusBar.foreground` `#CCCCCC`；`statusBarItem.hoverBackground` `#F1F1F133`（DM）；`statusBarItem.remoteBackground/Foreground`；`statusBar.border`              | S/M                   |
| 行为           | 左区：SCM / 分支 / 错误计数；右区：编码、光标行列、缩进、语言模式、通知铃铛、设置等；可配置 `workbench.statusBar.visible`、Remote 图标等                                                                                                                            | O userinterface       |

> 复用提示：RdataStation 状态栏是 "品牌色 + 白字"（SKILL），VS Code Dark Modern 用与侧栏同色的 
>
> `#181818`
>
>  灰而非品牌色；两种哲学都成立，但注意 
>
> `statusBar.background`
>
>  的 token 语义是 "可随上下文切换"（调试 / 远程 / 错误），RdataStation 若要做 "模式切换变色" 可直接抄这套 token 名。

### 2.6 面板 Panel



| 项        | 值                                                                                                                                                               | 来源                             |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| 头部（标题行）  | 高 **35px**（`.part > .title` 通用）                                                                                                                                 | S `part.css` + `panelpart.css` |
| 默认位置     | 底部（编辑器区下方），占编辑器区宽（Center 对齐）                                                                                                                                    | O custom-layout                |
| 高度       | 默认约编辑器区 40% 上下，可拖拽；**无固定默认像素（未实证）**                                                                                                                             | 未确认                            |
| 对齐       | Center / Justify / Left / Right（§1）                                                                                                                             | O                              |
| 最大化      | 右上角 chevron 展开占满编辑器区（Maximize Panel Size）                                                                                                                       | O                              |
| 颜色 token | `panel.background` / `panel.border`；`panelTitle.activeBorder`（顶 / 底 1px 指示条）、`activeForeground`、`inactiveForeground`；`panelSectionHeader.*`；`panelInput.border` | S/M                            |

### 2.7 分隔条 Sash



| 项         | 值                                                                                                                               | 来源                                          |
| --------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| 默认宽度 / 高度 | **4px**（`let globalSize = 4`）                                                                                                   | S `src/vs/base/browser/ui/sash/sash.ts:146` |
| hover 延迟  | **300ms**（`globalHoverDelay = 300`）                                                                                             | S 同上 :153                                   |
| 双击行为      | 双击复位布局（`onDidReset`，链接 sash 联动）                                                                                                 | S :595-607                                  |
| 光标        | 垂直：`ew-resize`（macOS `col-resize`）；水平：`ns-resize`（macOS `row-resize`）；边界态 `e/w/n/s-resize`；拖拽时全局 `* { cursor: ... !important }` | S                                           |
| 颜色        | 常规透明；hover 边框 `--vscode-sash-hoverBorder` = **focusBorder**（DM `#0078D4`）                                                       | S `miscColors.ts`                           |
| 触摸        | 支持 touch 拖拽与双指；双击（tap 间隔 250ms 判定）复位                                                                                            | S                                           |

### 2.8 窗口密度与布局自定义



| 版本                 | 内容                                                                                                                                                                                                                                                                                        | 来源                                                                                              |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| **v1.28**（2018-09） | Windows 默认启用自定义标题栏与可主题化菜单（`window.titleBarStyle` 默认 `custom`）                                                                                                                                                                                                                             | O [https://code.visualstudio.com/updates/v1\_28](https://code.visualstudio.com/updates/v1_28)   |
| **v1.83**（2023-09） | 新增 `window.density.editorTabHeight`：`default` / `compact`（经典 UI 下 35px / 22px）；新增 `workbench.editor.pinnedTabsOnSeparateRow`（pinned tab 单独成行）                                                                                                                                             | O v1\_83 + S `editorTabsControl.ts`                                                             |
| **v1.141**         | 新增 **Compact layout density**（`window.density.layout`）：更短标题栏、移除工作台部件间隙、减小 pane 间距；Agents 窗口可独立设置密度                                                                                                                                                                                        | O [https://code.visualstudio.com/updates/v1\_141](https://code.visualstudio.com/updates/v1_141) |
| 布局 CSS 变量          | core 实证：`--activity-bar-width`(48)、`--activity-bar-action-height`(48)、`--activity-bar-icon-size`(24)（`activitybarpart.css`/`activityaction.css`）；`--titlebar-height` **未见 core 消费**—— 标题栏高度由 TS 布局内联设置（35/30px），`--titlebar-height` 是社区扩展（APC customizeUI /lehni Custom UI Style）的约定变量【C】 | S + C                                                                                           |
| 窗口缩放               | `window.zoomLevel`：整档 20% 缩放根（`--window-zoom-factor`）；macOS 圆角 `calc(10px / var(--window-zoom-factor))` 反向抵消                                                                                                                                                                              | S/O                                                                                             |



***

## 3. 编辑器工作区

### 3.1 编辑器组结构与分割



| 项     | 值                                                                                                                                      | 来源                      |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| 容器    | 组容器 100% 高；标题行（tab / 面包屑）固定高，编辑器实例撑满剩余                                                                                                 | S `editorgroupview.css` |
| 空组    | `.empty { opacity: 0.5 }`（active/drag-over/ 最后组为 1）；焦点描边 `outline: 1px solid var(--vscode-editorGroup-focusedEmptyBorder)`、offset -2px | S                       |
| 组间分隔线 | `editorGroup.border`：DM `#FFFFFF17`（6% 白）                                                                                              | S/M                     |
| 组工具栏  | 多组 / 锁定时顶部工具条高 **35px**，action 间距 `margin-right: 4px`                                                                                  | S                       |
| 分割    | grid 网格左右 / 上下分割；拖 tab 到边缘生成新组；可拖出为浮动编辑器窗口；最小尺寸由内容（tab min-width 等）决定，CSS 不写死                                                          | S                       |
| 标题下边框 | 1px（`--title-border-bottom-color`）                                                                                                     | S                       |

### 3.2 编辑器 Tab

**高度**（【S】`editorTabsControl.ts` 常量 +【O】v1\_83）：



```
private static readonly EDITOR_TAB_HEIGHT = {
  normal: 35, compact: 22,        // 经典 UI：window.density.editorTabHeight default/compact
  modernUI: 32, modernUICompact: 28, // Modern UI：24/20px tab + 上下各 4px padding
  connected: 28, connectedCompact: 24,
};
```



| 档位                                        | 总高        | 说明                     |
| ----------------------------------------- | --------- | ---------------------- |
| `window.density.editorTabHeight: default` | **35px**  | 经典 UI 默认               |
| `compact`                                 | **22px**  | v1.83 引入，扩大编辑区         |
| Modern UI（default / compact）              | 32 / 28px | tab 本体 + 4px×2 padding |

高度写入 CSS 变量 `--editor-group-tab-height`，tab 容器 /tab 本体 / 面包屑 inline 行 /editor-actions 共用。

**宽度策略**（`workbench.editor.tabSizing`，【S】`multieditortabscontrol.css`）：



| 策略      | 规则                                                                                     |
| ------- | -------------------------------------------------------------------------------------- |
| fit（默认） | `width: 120px; min-width: fit-content; flex-shrink: 0` —— 120px 起，长内容撑宽，不收缩            |
| shrink  | `min-width: 80px; flex-basis: 0; flex-grow: 1; max-width: fit-content` —— 等分剩余，最小 80px |
| fixed   | `min-width: 50px; max-width: 160px`，等分，范围 50–160px                                     |

**其他规格**：



| 项             | 值                                                                                                                              | 来源                        |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------- |
| 左内距           | 10px（shrink/fixed 带图标且关闭钮在右时降为 5px）；label 渐变遮罩 5px                                                                             | S                         |
| 字号            | **13px**（`.title-label a`）                                                                                                     | S `editortabscontrol.css` |
| 关闭按钮          | 操作区宽 **28px**；图标 16×16、padding 2px；默认 opacity 0，active/hover/dirty 显示                                                          | S                         |
| dirty 指示      | 未保存显示实心圆点（`--vscode-icon-circle-filled-content`）；highlightModifiedTabs 模式：顶部 2px 色条（普通活动顶线 1px）                                | S                         |
| pinned/sticky | sticky-compact 固定 38px（仅图标）、sticky-shrink 80px；`position: sticky; z-index: 8`；pinned 分隔线 `tab.lastPinnedBorder`（Dark+ `#ccc3`） | S                         |
| 拖拽指示线         | 放置位 2px 蓝线（`--vscode-tab-dragAndDropBorder`）                                                                                   | S                         |

**颜色 token（Dark Modern 实测，【S】**`dark_modern.json`**）**：



| token                                           | DM 默认                        | 状态               |
| ----------------------------------------------- | ---------------------------- | ---------------- |
| `tab.activeBackground` / `activeForeground`     | `#1F1F1F` / `#FFFFFF`        | 活动 tab           |
| `tab.inactiveBackground` / `inactiveForeground` | `#181818` / `#9D9D9D`        | 非活动 tab          |
| `tab.hoverBackground`                           | `#1F1F1F`                    | hover（非活动）       |
| `tab.border`                                    | `#2B2B2B`                    | tab 间 / 底部分隔     |
| `tab.activeBorderTop`                           | `#0078D4`                    | 活动 tab 顶线（品牌蓝）   |
| `tab.activeBorder`                              | `#1F1F1F`                    | 活动 tab 底线（与背景融合） |
| `tab.selectedBackground` / `selectedForeground` | `#37373D` / `#FFFFFF`（Dark+） | 多选选中             |
| `tab.lastPinnedBorder`                          | `#ccc3`（Dark+）               | pinned 区右侧       |
| `editorGroupHeader.tabsBackground`              | `#181818`                    | tab 栏背景          |
| `editorGroupHeader.tabsBorder`                  | `#2B2B2B`                    | tab 栏底线          |

交互态：active → `box-shadow: inset var(--vscode-shadow-active-tab)`；`:hover:not(.active)` → `var(--vscode-shadow-sm)`；多选 `selected` 用 `tab-selected*`。未聚焦组用 `tab-unfocused*` token 族（DM 未单独覆盖，精确 hex 未确认）。

**行为约定**：拖拽排序（组内 / 组间 / 跨窗口）；中键关闭；右键 `EditorTitleContext` 菜单（复制路径 / 拆分 / 固定 / 关闭组）；预览 tab 斜体；双击标题空白新建 tab（组约定行为，绑定代码未逐行定位【未确认】）。

### 3.3 面包屑 Breadcrumb



| 项          | 值                                                                                                                                        | 来源                              |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| 行高（inline） | `line-height: var(--editor-group-tab-height)` = 35/22px（随 tab 档位）                                                                        | S `singleeditortabscontrol.css` |
| 分隔符        | `/`（Windows `\`），`font-size: 0.9em`；根目录 `\u00a0•\u00a0`                                                                                  | S                               |
| 文件类型徽标     | 字号 11px、padding `0 4px 0 8px`、max-width 40%；codicon 13px                                                                                 | S `breadcrumbscontrol.css`      |
| picker 条目  | 行高 **22px**；输入区高 **36px**、padding 5px 9px                                                                                                | S                               |
| 行内滚动条      | 覆盖式，默认 **3px** / large **8px**（SCROLLBAR\_SIZES）                                                                                         | S `breadcrumbsControl.ts`       |
| 颜色         | `breadcrumb.foreground` = foreground@0.8（DM ≈ `#CCCCCCCC`）；背景 = editor.background `#1F1F1F`；行下边框 1px `editorWidget.border`；聚焦项 underline | S `editorColors.ts` / css       |
| 行为         | 点击文件段开文件路径选择器、符号段开符号大纲选择器；段间 chevron-right 图标                                                                                            | S                               |

### 3.4 编辑器滚动条（Monaco 内部）



| 项         | 值                                                                                                                                                       | 来源                   |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 垂直宽 / 水平高 | **14px / 12px**（`editor.verticalScrollbarSize` / `horizontalScrollbarSize` 默认）                                                                          | S `editorOptions.ts` |
| 箭头        | 默认 11px（默认不渲染）                                                                                                                                          | S                    |
| token     | `scrollbarSlider.background` `#797979@0.4`、hover `#646464@0.7`、active `#BFBFBF@0.4`；轨道 `scrollbar.background` = null（透明）；滚动阴影 `scrollbar.shadow` `#000` | S `miscColors.ts`    |
| 显隐        | `auto`（溢出才显示，淡入淡出 100ms）                                                                                                                                | S                    |

> 注意两套滚动条：
>
> **编辑器内**
>
>  14/12px（Monaco），
>
> **工作台小部件**
>
> （树 / 列表 /quick input）10px（§4.6）。RdataStation 若分 "编辑器区 / 普通列表" 两套宽度，与 VS Code 一致。

### 3.5 Minimap



| 项         | 值                                                                                                                                     | 来源                   |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| gutter    | **8px**（`MINIMAP_GUTTER_WIDTH`）                                                                                                       | S `editorOptions.ts` |
| maxColumn | 默认 **120**                                                                                                                            | S                    |
| 宽度        | 动态：`min(maxColumn×charWidth, (剩余宽−滚动条−2)×缩放) + 8`；无固定默认 px                                                                            | S                    |
| 渲染比例      | 文本模式 2px / 行、块模式 3px / 行，× `editor.minimap.scale`（默认 1）                                                                               | S                    |
| token     | `minimap.background` = null（透明）；`minimap.foregroundOpacity` `#000f`；slider = scrollbarSlider@0.5；errorHighlight `rgba(255,18,18,0.7)` | S `minimapColors.ts` |
| 行为        | 点击跳转 / 拖 slider；slider 默认 mouseover 显示；size 三档 proportional/fill/fit                                                                  | S                    |

### 3.6 行号 Gutter



| 项     | 值                                                                                                                                                                                                                                  | 来源                   |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 预留位数  | `lineNumbersMinChars` 默认 **5**                                                                                                                                                                                                     | S `editorOptions.ts` |
| 列宽    | 随行数位数伸缩（maxDigitWidth 驱动）                                                                                                                                                                                                          | S                    |
| 行装饰区  | `lineDecorationsWidth` 默认 10px                                                                                                                                                                                                     | S                    |
| 字号    | 继承编辑器正文（默认 14px）                                                                                                                                                                                                                   | S                    |
| token | `editorLineNumber.foreground` `#6E7681`、`activeForeground` `#CCCCCC`（DM）；`editorGutter.modifiedBackground` `#0078D4`、`addedBackground` `#2EA043`、`deletedBackground` `#F85149`；`editor.lineHighlightBackground` DM 未覆盖（透明）【未确认精确值】 | S/M                  |

### 3.7 编辑器字号基线

【S】`fontInfo.ts` `EDITOR_FONT_DEFAULTS`：



| 项                   | 默认                                                                |
| ------------------- | ----------------------------------------------------------------- |
| `editor.fontSize`   | **14px**（Win/Linux）、**12px**（macOS）                               |
| `editor.lineHeight` | 0（自动）：非 Mac `round(14×1.35)=19px`、Mac `round(12×1.5)=18px`；下限 8px |
| 字体（Win）             | `Consolas, 'Courier New', monospace`                              |

### 3.8 Watermark（空编辑器欢迎语）



| 项     | 值                                                                               | 来源                      |
| ----- | ------------------------------------------------------------------------------- | ----------------------- |
| 内容最大宽 | **272px**（辅助窗口 200px）                                                           | S `editorgroupview.css` |
| logo  | 最大 **256px**，随主题切 4 套 SVG                                                       | S                       |
| 区块间距  | gap **24px**                                                                    | S                       |
| 顶部工具条 | 高 35px、padding 0 8px                                                            | S                       |
| 快捷键提示 | 文字 `--vscode-descriptionForeground`（DM `#9D9D9D`）；`dd` margin-inline-start 24px | S                       |
| 显示条件  | 仅无编辑器时；宽 ≤478px 隐藏快捷键区                                                          | S                       |



***

## 4. 通用小部件（Widgets）

### 4.1 树 Tree / 列表 List



| 项                       | 值                                                                                                                                                                                                                                                                                                  | 来源              |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------- |
| 行高                      | **22px**（`treeView.ts:1246` `static readonly ITEM_HEIGHT = 22`；quickInput 行同 22px 旁证）                                                                                                                                                                                                              | S               |
| 缩进槽                     | `.monaco-tl-indent { left: 16px }`（无箭头树左移 12px）                                                                                                                                                                                                                                                    | S `tree.css`    |
| 箭头槽                     | 宽 **16px**、padding-right 6px、translateX 3px、图标 **10px**；收起 rotate (-90deg)                                                                                                                                                                                                                         | S               |
| 缩进参考线                   | 1px 竖线、默认 opacity 0、hover 当前行淡入                                                                                                                                                                                                                                                                    | S               |
| `workbench.tree.indent` | **未确认**（配置默认未在已抓源码定位；社区常记 8px 档）                                                                                                                                                                                                                                                                   | 未确认             |
| checkbox                | **未确认**（widget 来自 `base/browser/ui/toggle`，CSS 未取到）                                                                                                                                                                                                                                                | 未确认             |
| token                   | 见 §5.3：`list.activeSelectionBackground` 暗 `#04395E`（R 默认）/ DM 主题链 `#FFFFFF22`、`inactiveSelectionBackground` `#37373D`、`hoverBackground` `#2A2D2E`、`focusBackground` null、`focusOutline`=focusBorder、`highlightForeground` `#2AAAFF`、`dropBackground` `#062F4A`、`tree.indentGuidesStroke` `#585858` | S               |
| 交互                      | active/inactive 语义 = 键盘焦点在 / 不在；hover 显示 `list.hoverBackground`（延迟阈值未确认）；focused 行外圈 `list.focusOutline` 1px；disabled 行 cursor: default                                                                                                                                                            | S/O             |
| 键盘                      | ↑/↓ 移动焦点、→ 展开、← 收起、单击 / Enter 选中                                                                                                                                                                                                                                                                   | O userinterface |

### 4.2 输入框 Input Box



| 项     | 值                                                                                                                                                                                                                                     | 来源               |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| 内距    | **4px 6px**（input 与 mirror 层）                                                                                                                                                                                                         | S `inputBox.css` |
| 圆角    | **4px**                                                                                                                                                                                                                               | S                |
| 高度    | 无固定值（内容撑开，13px 继承下观感 ≈24px）；树内过滤输入框固定 **23px**、内距 2px 4px                                                                                                                                                                             | S                |
| 字号    | 继承（13px）；校验消息 12px / 行高 17px                                                                                                                                                                                                          | S                |
| 校验块   | padding 0.4em、margin-top -1px                                                                                                                                                                                                         | S                |
| 动作区   | right 2px / top 4px，codicon 16px                                                                                                                                                                                                      | S                |
| token | `input.background` `#3C3C3C`（R）/ DM `#313131`、`input.border` null（R）/ DM `#3C3C3C`、`placeholderForeground` foreground@0.5、`inputOption.activeBorder` `#007ACC`、`inputValidation.error*` `#5A1D1D`/`#BE1100`（R）、DM `#3A1D1D`/`#BE1100` | S                |
| focus | 1px `focusBorder` 描边（全局规则：`outline: 1px solid var(--vscode-focusBorder); outline-offset: -1px`）                                                                                                                                       | S `style.css`    |

### 4.3 按钮 Button



| 项        | 值                                                                                                                                                                                                             | 来源             |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| 内距       | **4px 8px**（标准）；`.small`：3px 6px                                                                                                                                                                              | S `button.css` |
| 圆角       | **4px**                                                                                                                                                                                                       | S              |
| 字号 / 行高  | **12px / 16px**；小按钮 11px/14px                                                                                                                                                                                 | S              |
| 推算高度     | ≈ **26px**（4+4+16 + 边框 1px×2，border-box）                                                                                                                                                                      | S 推算           |
| 边框       | `1px solid var(--vscode-button-border, transparent)`                                                                                                                                                          | S              |
| 图标       | codicon，margin 0 0.2em                                                                                                                                                                                        | S              |
| disabled | `opacity: 0.4; cursor: default`                                                                                                                                                                               | S              |
| focus    | outline + `outline-offset: 2px`                                                                                                                                                                               | S              |
| token    | `button.background` `#0E639C`（R）/ DM `#0078D4`（2026 `#297AA0`）；`foreground` `#FFFFFF`；`hoverBackground` `lighten(#0E639C,0.2)`（R）/ DM `#026EC1`；`secondaryBackground` `#2A2D2E`（R）/ DM 透明 + `secondaryBorder` | S              |

### 4.4 下拉菜单 Dropdown



| 项         | 值                                                                                                                                                                                                                                                               | 来源               |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| 触发器       | `height: 100%`（随所在行高）、padding 0、flex 居中                                                                                                                                                                                                                         | S `dropdown.css` |
| 弹层        | 圆角 `--vscode-cornerRadius-large`、阴影 `--vscode-shadow-lg`；宽度随内容                                                                                                                                                                                                  | S                |
| 单行行高 / 内距 | **未确认**（复用 `.monaco-menu`，menu.css 未取到）                                                                                                                                                                                                                         | 未确认              |
| token     | `dropdown.background` `#3C3C3C`（R）/ DM `#313131`、`dropdown.listBackground` null（R）/ DM `#1F1F1F`、`dropdown.border` = background（R）/ DM `#3C3C3C`；`menu.background` `#252526`（Dark+）/ DM `#1F1F1F`、`menu.selectionBackground` `#0078d4`（Dark+）/ DM 链 `#3994BC26` | S                |

### 4.5 徽标 Badge（专项）

**组件本体**（`.monaco-count-badge`，【S】`src/vs/base/browser/ui/countBadge/countBadge.css`）：



| 项                | 值                                                         |
| ---------------- | --------------------------------------------------------- |
| 内距               | **3px 5px**（上下 3 / 左右 5）                                  |
| 圆角               | **11px（胶囊形，不是 2px 直角条）**                                  |
| 字号               | **11px**、line-height 11px、字重 normal                       |
| 最小尺寸             | **min-width: 18px; min-height: 18px**（数字徽章最小 18×18 正方形胶囊） |
| 长文本变体 `.long`    | padding 2px 3px、圆角 **2px**、高度随内容                          |
| Quick Input 计数变体 | padding 2px 4px、圆角 2px                                    |

**纠偏**：常见社区描述 "padding 0 4px / 圆角 2px" 是错的；圆角 2px 只出现在 `.long` 与 Quick Input 计数变体，标准徽标是 18×18 圆角 11px 胶囊。

**颜色 token**（【S】`miscColors.ts` + 主题 JSON）：



| token                                                               | 注册表暗色默认             | Dark Modern | 2026-dark             |
| ------------------------------------------------------------------- | ------------------- | ----------- | --------------------- |
| `badge.background`                                                  | `#4D4D4D`           | `#616161`   | `#307E9F`             |
| `badge.foreground`                                                  | `#FFFFFF`           | `#F8F8F8`   | `#FFFFFF`             |
| `activityBarBadge.background`                                       | `#007ACC`（dark\_vs） | `#0078D4`   | `#307E9F`             |
| `activityBarBadge.foreground`                                       | `#FFFFFF`           | `#FFFFFF`   | `#FFFFFF`             |
| `activityWarningBadge.background` / `activityErrorBadge.background` | —                   | —           | `#CCA700` / `#f48771` |

**使用场景与尺寸差异**：



| 场景               | 尺寸特征                                                       |
| ---------------- | ---------------------------------------------------------- |
| 活动栏 item 角标      | 专用蓝底（activityBarBadge.\*）；活动栏容器内高 16px、圆角 20px、9px 字（§2.3） |
| 资源管理器 / 侧边栏计数    | 标准 18×18 胶囊、11px 字（badge.\*）                               |
| 搜索结果计数           | 标准 badge.\*（官方文档明示场景）                                      |
| Quick Input 右侧计数 | 2px 4px、圆角 2px 直角小条                                        |
| 长文本徽标            | `.long`：2px 3px、圆角 2px                                     |

### 4.6 滚动条（工作台小部件）



| 项        | 值                                                                                                                                             | 来源                              |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| 槽位 / 滑块宽 | **10px**（`verticalScrollbarSize` / `verticalSliderSize` 默认；水平同 10px）                                                                          | S `scrollableElementOptions.ts` |
| 显隐       | `auto`（hover 显示）；显示淡入 **100ms**、隐藏淡出 **800ms**                                                                                                | S `scrollbars.css`              |
| 圆角       | CSS 未设（矩形滑块）                                                                                                                                  | S                               |
| 拖拽细节     | 指针偏离滑块正交 >**140px** 重置拖动（Windows 行为）                                                                                                          | S `abstractScrollbar.ts`        |
| token    | `scrollbarSlider.background` `rgba(121,121,121,.4)`、hover `rgba(100,100,100,.7)`、active `rgba(191,191,191,.4)`（R）；DM 链 2026 `#A8A9AA85/90/9C` | S                               |

### 4.7 Quick Input（命令面板 / 快速选择）



| 项       | 值                                                                                                                                                                                                              | 来源                      |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| 面板宽     | **600px**                                                                                                                                                                                                      | S `quickInput.css:6-14` |
| 圆角 / 阴影 | `cornerRadius-xLarge` / `shadow-xl`；z-index 2550                                                                                                                                                               | S                       |
| 行高      | **22px**；列表 max-height **440px**（20 行）                                                                                                                                                                         | S                       |
| 条目内距    | `0 6px`；行圆角 3px                                                                                                                                                                                                | S                       |
| 图标槽     | 16px 宽、右内距 6px                                                                                                                                                                                                 | S                       |
| 分组行     | padding 4px 6px、12px、600                                                                                                                                                                                       | S                       |
| 动画      | 顶部居中弹出，250ms 缩放淡入                                                                                                                                                                                              | S                       |
| token   | `quickInput.background` = editorWidget 系（R `#1E1E1E`）/ DM `#222222`（2026 `#202122`）；`quickInputList.focusBackground` = listActiveSelection（R）/ `#297AA0`；`pickerGroup.foreground` `#3794FF`、`border` `#3F3F46` | S                       |
| 行为      | 输入即模糊过滤；↑↓ 循环；Tab 补全；Esc 关闭；条目右动作 hover/focus 显现                                                                                                                                                               | S/O                     |

### 4.8 通知 Notification（Toast / 通知中心）



| 项           | 值                                                                                                                                                                        | 来源                            |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------- |
| Toast 最大宽   | **450px**（`MAX_WIDTH = 450`，不是 400）                                                                                                                                      | S `notificationsToasts.ts:50` |
| 位置          | 右下：`right: 3px; bottom: 25px`（22px 状态栏 + 3px）                                                                                                                            | S                             |
| 条目内距        | **10px 5px**                                                                                                                                                             | S `notificationsList.css`     |
| 图标槽         | 16×22px、图标 18px                                                                                                                                                          | S                             |
| 消息行高 / 来源字号 | 22px / 12px                                                                                                                                                              | S                             |
| 工具条         | 高 22px、默认隐藏、hover/focus 显示                                                                                                                                               | S                             |
| 动画          | 300ms ease-out 位移 + 透明度                                                                                                                                                  | S                             |
| z-index     | toasts 2545（遮罩 2540 /quick input 2550 /dialog 2575 之下）                                                                                                                   | S                             |
| token       | `notifications.background` DM `#1F1F1F`（2026 `#202122`）、foreground `#CCCCCC`、border `#2B2B2B`；icon 语义 error `#f48771` / warning `#CCA700` / info `#3a94bc`；注册表默认未定位【未确认】 | S/M                           |

### 4.9 对话框 Dialog



| 项       | 值                                                                                                            | 来源                   |
| ------- | ------------------------------------------------------------------------------------------------------------ | -------------------- |
| 最小宽     | **480px**（竖向对齐变体 350px）                                                                                      | S `dialog.css`       |
| 最大      | 90vw / 90vh；最小高 75px；容器 padding 8px                                                                          | S                    |
| 圆角 / 阴影 | `cornerRadius-xLarge` / `shadow-xl`；遮罩 `rgba(0,0,0,0.5)`、z-index 2575                                        | S                    |
| 图标      | 24px（竖向变体 64px）                                                                                              | S                    |
| 主消息     | **14px / 600**、min-height 22px、mb 4px                                                                        | S                    |
| 按钮区     | margin-top 20px、按钮 margin 4px、右对齐                                                                            | S                    |
| token   | **无&#x20;**`dialog.*`**&#x20;专属 token**；底色沿用 `editorWidget.background`，按钮复用 `button.*`，focus 用 `focusBorder` | O（theme-color 核对）+ S |



***

## 5. 主题与颜色 token 体系

### 5.1 机制总览



* **主题 JSON**：`type`（vs / vs-dark / hc-black / hc-light）+ `include` 链式继承 + `colors`（UI token）+ `tokenColors`（语法高亮）。`dark_plus.json`/`light_plus.json` **只含 tokenColors**，UI 色在 `dark_vs/light_vs` 与 `*_modern` 覆盖层。当前主题链：`2026-dark → dark_modern → dark_plus → dark_vs`【S】。

* **自定义**：`workbench.colorCustomizations` 优先级最高（可按主题限定 `"[Dark Modern]": {...}`）【O themes 文档】。

* **高对比**：`contrastBorder`（hcDark `#6FC3DF` / hcLight `#0F4A85`）与 `contrastActiveBorder` 仅 HC 主题生效，常规主题 null【S `baseColors.ts`】。

* **CSS 变量注入**：token `a.b.c` → CSS 变量 `--vscode-a-b-c`，由平台主题服务注入 `.monaco-workbench` 根；阴影等非色 token 也提升为 `--vscode-shadow-sm/md/lg/xl`；另有新设计 token：`--vscode-cornerRadius-medium/large/xLarge`、`--vscode-bodyFontSize-xSmall`、`--vscode-spacing-size40` 等（Quick Input / Dialog / 下拉已在消费）【S】。

* 图标规则由 `iconsStyleSheet.ts` 动态生成（codicon.css 注释实证）【S】。

### 5.2 token 家族清单（按官方分组）



* **全局**：`focusBorder`、`contrastBorder`、`contrastActiveBorder`、`foreground`、`descriptionForeground`、`errorForeground`、`icon.foreground`、`selection.background`、`textLink.*`、`textSeparator.*`、`textPreformat.*`、`textBlockQuote.*`、`textCodeBlock.background`

* **Widget**：`widget.shadow`、`widget.border`

* **Editor**：`editor.background/foreground/selectionBackground/inactiveSelectionBackground/selectionHighlightBackground/lineHighlightBackground`；`editorWidget.*`、`editorError/Warning/Info.*`；语法走 tokenColors（dark：comment `#6A9955`、string `#ce9178`、keyword `#569cd6`）

* **Line/Gutter/Minimap**：`editorLineNumber.*`、`editorGutter.*`、`editorIndentGuide.*`、`minimap.*`、`minimapSlider.*`

* **Tabs/Groups**：`editorGroup.border`、`editorGroupHeader.*`、`tab.*`（含 `tab.selected*`、`tab.lastPinnedBorder`）

* **Breadcrumb**：`breadcrumb.foreground/background/focusForeground/activeSelectionForeground`、`breadcrumbPicker.background`

* **Input/Button/Dropdown/Checkbox/Badge**：`input.*`、`inputOption.*`、`inputValidation.*`、`button.*`（含 secondary\*、separator）、`dropdown.*`、`checkbox.*`、`badge.*`

* **List/Tree**：`list.*`（active/inactive/hover/focus/drop/highlight/error/warning）、`listFilterWidget.*`、`tree.indentGuidesStroke`、`tree.inactiveIndentGuidesStroke`、`tree.table*`

* **Menu**：`menu.*`（background/foreground/selectionBackground/selectionForeground/separatorBackground/border）、`menubar.selectionBackground/Foreground/Border`

* **Activity Bar**：`activityBar.*`（含 `activeBorder/activeBackground/activeFocusBorder/inactiveForeground/dropBorder`）、`activityBarBadge.*`、`activityWarningBadge.*`、`activityErrorBadge.*`；modern UI：`modernActivityBar.*`、`modernActivityBarItem.*`

* **Status Bar**：`statusBar.*`（含 debuggingBackground/noFolderBackground）、`statusBarItem.*`（hover/active/prominent/remote/error/warning）

* **Title Bar**：`titleBar.activeBackground/activeForeground/inactiveBackground/inactiveForeground/border`；`commandCenter.*`

* **Side Bar / Panel**：`sideBar.*`、`sideBarTitle.*`、`sideBarSectionHeader.*`；`panel.*`、`panelTitle.*`、`panelSectionHeader.*`、`panelInput.border`

* **Notification**：`notifications.*`（background/foreground/border）、`notificationCenterHeader.*`、`notificationToast.border`、`notificationLink.foreground`、`notifications{Error,Warning,Info}Icon.foreground`（注：现行前缀为 `notifications`，旧文档 `notification`）

* **Quick Input / Picker**：`quickInput.background/foreground`、`quickInputTitle.background`、`quickInputList.*`、`pickerGroup.*`

* **Scrollbar / Sash / Progress**：`scrollbar.shadow`、`scrollbarSlider.*`、`sash.hoverBorder`、`progressBar.background`

* **Dialog / Git / Problems**：`dialog.*`（官方页列有 `dialog.background/foreground/border/button.*`，但对话框组件实测**不消费** dialog token）、`gitDecoration.*`、`problemsErrorIcon.foreground` 等

### 5.3 关键 token 默认值表（暗 × 亮）

> 暗 = Dark Modern 生效值；亮 = Light Modern 生效值；M = 主题 JSON，R = 
>
> `registerColor`
>
>  源码默认（主题未覆盖时回退）。色值全部来自 jsdelivr 直取的 JSON/TS，非记忆。



| Token                                            | 暗                       | 亮                       | 来源                              |
| ------------------------------------------------ | ----------------------- | ----------------------- | ------------------------------- |
| `foreground`                                     | `#CCCCCC`               | `#3B3B3B`               | M（R 原始暗 `#CCCCCC`/ 亮 `#616161`） |
| `descriptionForeground`                          | foreground@0.7          | `#717171`               | R baseColors.ts                 |
| `icon.foreground`                                | `#CCCCCC`               | `#3B3B3B`               | M（R 暗 `#C5C5C5`/ 亮 `#424242`）   |
| `focusBorder`                                    | `#0078D4`               | `#005FB8`               | M（R 暗 `#007FD4`/ 亮 `#0090F1`）   |
| `badge.background`                               | `#616161`               | `#CCCCCC`               | M（R 暗 `#4D4D4D`/ 亮 `#C4C4C4`）   |
| `badge.foreground`                               | `#F8F8F8`               | `#3B3B3B`               | M（R 暗 `#FFFFFF`）                |
| `button.background`                              | `#0078D4`               | `#005FB8`               | M（R `#0E639C`）                  |
| `button.hoverBackground`                         | `#026EC1`               | `#0258A8`               | M                               |
| `button.secondaryBackground`                     | `#00000000`             | `#E5E5E5`               | M                               |
| `input.background` / `input.border`              | `#313131` / `#3C3C3C`   | `#FFFFFF` / `#CECECE`   | M（R `#3C3C3C` / null）           |
| `input.placeholderForeground`                    | `#989898`               | `#767676`               | M                               |
| `list.activeSelectionBackground`                 | `#04395E`               | `#E8E8E8`               | R（亮被 modern 覆盖）                 |
| `list.activeSelectionForeground`                 | `#FFFFFF`               | `#000000`               | R/M                             |
| `list.inactiveSelectionBackground`               | `#37373D`               | `#E4E6F1`               | R                               |
| `list.hoverBackground`                           | `#2A2D2E`               | `#F2F2F2`               | R/M                             |
| `list.focusBackground`                           | null                    | null                    | R                               |
| `list.highlightForeground`                       | `#2AAAFF`               | 亮 `#0066BF`             | R                               |
| `tree.indentGuidesStroke`                        | `#585858`               | `#a9a9a9`               | R                               |
| `activityBar.background` / `foreground`          | `#181818` / `#D7D7D7`   | `#F8F8F8` / `#1F1F1F`   | M                               |
| `activityBarBadge.background` / `foreground`     | `#0078D4` / `#FFFFFF`   | `#005FB8` / `#FFFFFF`   | M（R 暗 `#007ACC`）                |
| `statusBar.background` / `foreground`            | `#181818` / `#CCCCCC`   | `#F8F8F8` / `#3B3B3B`   | M                               |
| `statusBarItem.hoverBackground`                  | `#F1F1F133`             | `#1F1F1F11`             | M                               |
| `titleBar.activeBackground` / `activeForeground` | `#181818` / `#CCCCCC`   | `#F8F8F8` / `#1E1E1E`   | M                               |
| `sideBar.background` / `foreground`              | `#181818` / `#CCCCCC`   | `#F8F8F8` / `#3B3B3B`   | M                               |
| `sideBarSectionHeader.background`                | `#181818`               | `#F8F8F8`               | M                               |
| `editor.background` / `foreground`               | `#1F1F1F` / `#CCCCCC`   | `#FFFFFF` / `#3B3B3B`   | M（R 暗 `#1E1E1E`/`#BBBBBB`）      |
| `editorWidget.background`                        | `#202020`               | `#F8F8F8`               | M（R 暗 `#252526`）                |
| `breadcrumb.foreground`                          | foreground@0.8          | foreground@0.8          | R                               |
| `minimap.background`                             | null                    | null                    | R                               |
| `notifications.background` / `foreground`        | `#1F1F1F` / `#CCCCCC`   | `#FFFFFF` / `#3B3B3B`   | M                               |
| `quickInput.background` / `foreground`           | `#222222` / `#CCCCCC`   | `#F8F8F8` / `#3B3B3B`   | M                               |
| `scrollbarSlider.background`                     | `#797979@0.4`           | `#646464@0.4`           | R                               |
| `scrollbarSlider.hoverBackground`                | `#646464@0.7`           | `#646464@0.7`           | R                               |
| `scrollbarSlider.activeBackground`               | `#BFBFBF@0.4`           | `#000000@0.6`           | R                               |
| `sash.hoverBorder`                               | = focusBorder `#0078D4` | = focusBorder `#005FB8` | R                               |
| `progressBar.background`                         | `#0E70C0`               | `#0E70C0`               | R（M 覆盖 `#0078D4`/`#005FB8`）     |

### 5.4 暗色分层灰阶（层级靠背景明度梯度）

`#181818`（titleBar/sideBar/statusBar/tabBar 底）→ `#1F1F1F`（editor /tab 活动 /menu）→ `#222222`（quickInput）→ `#313131`（input/dropdown）→ `#202020`（editorWidget）。亮色：`#F8F8F8`（chrome）→ `#FFFFFF`（editor/input）。

Modern 强调色家族统一 `#0078D4`（focusBorder / button / activityBarBadge / progressBar / tab.activeBorderTop）。



***

## 6. 图标体系 Codicon 与字体基线

### 6.1 Codicon



| 项          | 值                                                                                                                                                                                                                                                                             | 来源                            |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| 默认渲染字号     | **16px**：`font: normal normal normal 16px/1 codicon`；workbench 再强调 `.monaco-workbench .codicon { font-size: 16px }`                                                                                                                                                           | S `codicon.css` + `style.css` |
| 机制         | 图标字体（codicon.ttf），Unicode 码位 + 类名 `codicon-<name>`；SVG sprite 备用；规则由 iconsStyleSheet.ts 动态生成                                                                                                                                                                                  | S                             |
| 修饰         | `codicon-modifier-spin` 等（完整修饰类清单未逐条抓取【未确认】）                                                                                                                                                                                                                                  | O icons 页                     |
| 源 SVG 设计网格 | 渲染 16px 已实证；**源 SVG viewBox 网格（社区传 24×24）未实证【未确认】**                                                                                                                                                                                                                           | 未确认                           |
| 场景尺寸       | 活动栏 24px（§2.3 实证）；下拉箭头 16px；标题栏 / 树 / 菜单 / 状态栏 /tab 图标默认走 16px 基线（逐控件覆盖值未逐一实证【未确认】）                                                                                                                                                                                           | S                             |
| 官方入口       | API：[https://code.visualstudio.com/api/references/icons](https://code.visualstudio.com/api/references/icons) ；预览：[https://microsoft.github.io/vscode-codicons/dist/codicon.html](https://microsoft.github.io/vscode-codicons/dist/codicon.html) ；仓库 microsoft/vscode-codicons | O                             |

### 6.2 UI 字体字号基线



* 根 **13px**（`.monaco-workbench`）、body 11px、行高 1.4em（§1）。

* 字号档分布：11px（body 元信息、分区头 h2、面板 / 状态栏内小元素）、12px（状态栏、按钮、pickerGroup、来源文本）、13px（根 / 菜单 /tab/ 树默认继承）、14px（编辑器正文 Win）。

* 编辑器文本走独立 `editor.fontSize`（14/12px，等宽字体，§3.7）。

* `window.zoomLevel` 整档 20% 缩放。



***

## 7. 交互与行为约定汇总（跨组件）



| 约定          | 值                                                                                                              | 来源                         |
| ----------- | -------------------------------------------------------------------------------------------------------------- | -------------------------- |
| focus 环（全局） | `outline: 1px solid var(--vscode-focusBorder); outline-offset: -1px`；按钮例外 offset 2px；树聚焦行用 `list.focusOutline` | S `style.css`/`button.css` |
| hover 延迟    | sash 300ms（sash.ts）；列表 hover 延迟常量未确认；scrollbar 淡入 100ms / 淡出 800ms                                             | S                          |
| 键盘通用        | ↑↓←→ 导航、Enter 确认、Esc 取消 / 关闭、Tab 补全（quick input）；Alt 聚焦菜单栏                                                     | O userinterface            |
| 双击语义        | sash 双击复位布局；tab 双击空白新建（约定）；通知展开                                                                                | S/O                        |
| 中键语义        | tab 中键关闭                                                                                                       | S                          |
| 拖拽          | tab 跨组 / 跨窗口拖拽（2px 指示线）；视图 / 面板跨区拖放（list.dropBackground 落点）；滚动条偏离 >140px 重置                                    | S                          |
| z-index 层级  | sash 35 → 通知遮罩 2540 → toast 2545 → quick input 2550 → dialog 2575（从小到大盖在上面）                                    | S                          |
| 动画          | quick input 250ms 缩放淡入；toast 300ms；滚动条 100/800ms                                                               | S                          |
| 密度          | 两档体系：`window.density.editorTabHeight`（35/22）+ `window.density.layout` compact（短标题栏、去部件间隙、减 pane 间距）            | O                          |



***

## 8. VS Code ↔ RdataStation 复用对照建议

RdataStation 现状（来源：`.agents/skills/rds-ui-spec/SKILL.md`，16px rem 基准；`RD` = 现状值，`VS` = VS Code 值）：



| 用途          | VS Code（13px 基准）                                   | RdataStation（16px 基准）          | 判定                                                                   |
| ----------- | -------------------------------------------------- | ------------------------------ | -------------------------------------------------------------------- |
| 活动栏宽        | 48px                                               | 48px（3rem）                     | **同值，直接对齐**                                                          |
| 标题栏高        | 30/35px                                            | 36px（2.25rem）                  | 已对齐（35→36 仅 +1px）                                                    |
| 活动栏 item    | 48×48px 正方                                         | 44×40px                        | **差异保留**（RdataStation 扁形为产品选择）                                       |
| 活动栏图标       | 24px                                               | 28px（1.75rem）                  | **差异保留**（28px 更大更易点）                                                 |
| 行高（树 / 列表）  | 22px                                               | 24px（1.5rem）                   | 接近；按换算 ≈27px，24px 为紧凑取舍，**可保留**                                      |
| 控件（输入 / 按钮） | 输入 ≈24、按钮 ≈26px                                    | 32 / 26px                      | 按钮 26↔32（MD 档）已对齐换算；输入 24→30，RD 32/26 两档覆盖                           |
| 面板头 / 分区头   | 35 / 22px                                          | 36 /（树内无分区头概念）                 | 已对齐                                                                  |
| 状态栏高        | 22px                                               | 无独立常量（品牌色 + 白字）                | **建议登记**：22→27px（1.7rem）或沿用 24px 行高档                                 |
| Tab 头高      | 35px（default）/ 22px（compact）                       | 36px（2.25rem）                  | 已对齐 default 档；compact 档建议 28px（对应 VS Modern 28 而非 22，16px 基准下 22 过小） |
| 徽标          | 18×18 胶囊、圆角 11、11px                                | 无独立规格                          | **建议引入 VS 胶囊规格**（含 `.long` 2px 变体）                                   |
| 滚动条         | 工作台 10px / 编辑器 14/12px                             | 无独立规格                          | 建议工作台 10px、编辑 / 表格区 12–14px 双档                                       |
| 分隔条         | 4px、双击复位、hover 300ms                               | 未单列                            | 建议 4px + 双击复位                                                        |
| 缩进公式        | 箭头槽 16 + 每级（tree.css 左移 16；配置 8 档未确认）              | 0.5rem(8) + 0.875rem(14)×depth | 结构同型，RD 保留（14px 步进偏大但稳定）                                             |
| 间距          | 4/8/12（按钮 4/8、badge 3/5、toast margin 4、picker 4/6） | 4/8/12（GAP\_SM/MD/LG）          | **同构**，可直接对齐语义                                                       |
| 描边          | 1px（border/focus）                                  | 1px（HAIRLINE）+2px 激活条          | 同构                                                                   |
| 根字号         | 13px                                               | 16px（rem）                      | **基准差异，不互抄**（见 8.3）                                                  |
| 强调色         | `#0078D4` 家族（focus/button/badge/progress/tab 顶线）   | 待查（rds-theme）                  | 建议统一一个 accent token 家族                                               |
| 暗色层级        | `#181818→#1F1F1F→#222222→#313131`                  | 待查（rds-theme）                  | 建议直接采 Dark Modern 灰阶                                                 |

### 8.1 可直接对齐（无需换算）



1. **活动栏宽 48px**（两边同值）。

2. **标题栏 / 面板头 / Tab 头 ≈35–36px 体系**：RdataStation 36px 与 VS Code 35px（命令中心态）语义相同。

3. **徽标胶囊规格**：18×18 最小、内距 3/5、圆角 11px、11px 字号；长文本 `.long` 2px 圆角变体。可写进 gpui-kit 的 Badge 组件规格。

4. **focus 环**：1px + focusBorder token + outline-offset -1px（按钮 2px）。

5. **分隔条**：4px、hover 300ms、双击复位。

6. **状态栏 item 内距 0 5px / 间距 3px**、hover 底色 token 语义。

7. **间距语义同构**：4/8/12 档与 GAP\_SM/MD/LG 一一对应；对话框按钮 margin 4px、watermark gap 24px 等均可照用。

8. **Dark Modern 暗色灰阶与强调色家族**：`#181818/#1F1F1F/#222222/#313131` + `#0078D4`（暗）/ `#005FB8`（亮）—— 可作为 rds-theme 的主题基线（比旧 Dark+ 更现代，且 `2026-dark` 链正在其上继续演进）。

9. **两套滚动条宽度**（工作台 10px / 内容区 12–14px）与 100/800ms 淡入淡出。

10. **树缩进结构**：基准 padding + 每级步进 × depth（VS 与 RD 是同一种公式）。

### 8.2 需按 rem 体系换算（13px → 16px 基准，系数 ×16/13 ≈ 1.23，圆整到 4px 网格）



| VS Code 值      | ×1.23 | 建议 @16px          | 对应 RD 现有                  |
| -------------- | ----- | ----------------- | ------------------------- |
| 行高 22          | 27    | 28（1.75rem）或保留 24 | 24                        |
| 按钮高 ≈26        | 32    | 32                | 32（CONTROL\_HEIGHT\_MD）✓  |
| 输入高 ≈24        | 30    | 30（1.875rem）      | 32 / 26                   |
| 状态栏 22         | 27    | 28（1.75rem）       | —                         |
| Tab compact 22 | 27    | 28                | 36（default 档）             |
| 命令中心 22        | 27    | 28                | Quick Open 26             |
| 菜单按钮行高 30      | 37    | 36                | 标题栏 36 ✓                  |
| 侧边栏分区头 22      | 27    | 28                | —                         |
| 徽标字号 11        | 13.5  | 13–14（text\_sm）   | text\_xs 12 / text\_sm 14 |
| 树箭头图标 10       | 12.3  | 12                | ICON\_SIZE\_XS 12 ✓       |
| 状态栏字号 12       | 14.8  | 14–15             | text\_sm 14               |
| Tab 字号 13      | 16    | 15–16             | text\_sm/base             |

换算后的落点大多落在 RD 现有档位（32/36/12/14/16）附近，说明当前 UI.rs 常量体系与 VS Code 在**同一密度区间**，主要是把 "换算依据" 登记进 ui.rs 注释，供后续取值时引用。

### 8.3 不宜照搬



1. **13px 基准本身**：VS Code 的绝对 px 全建立在 13px 上；RdataStation 是 16px rem，视图层禁止裸 `px(N.)`（SKILL 硬约束）。所有 VS Code 数值只能以 "参考 → 换算 → 圆整" 方式落地，不能直接抄。

2. **11px 元信息字号**（body / 分区头 h2）：16px 基准下最小可读字号应为 12px（text\_xs），11px 不要引入。

3. **活动栏 item 48×48 与图标 24px**：RD 的 44×40 + 28px 是更大的命中区 / 图标，是桌面工具的正确取向（VS Code 面向多窗口浏览器场景做了妥协）；不要为 "对齐" 缩小。

4. **活动栏徽标 9px 字号**：16px 基准下 9px 不可读；RD 若做活动栏角标用 ≥10–11px。

5. **紧凑密度极端档**（tab 22px、行高 22px）：16px 基准下 ≈27px 已是下限，22px 直接不可用；RD 的 compact 应取 28px 档。

6. **Windows 原生窗口控制 / 自定义标题栏实现**：Tauri 自带窗口装饰与拖拽区，不复制 VS Code 的 window-controls DOM/CSS 机制；只参考 " 关闭按钮 hover 红色 `rgba(232,17,35,.9)`" 这类视觉语义。

7. **组件行为归属**：树 / 列表 / 按钮 / 下拉的交互语义（hover 延迟、选中态、键盘导航、拖拽吸附 140px、tab 双行、z-index 栈）属于 **gpui-kit 组件行为**，不应为了 "对齐 VS Code" 在视图层重写；行为文档写进 `docs/architecture/theme/ui-constraints.md` 供组件层实现参考即可。

8. **Monaco 专属体系**：minimap、行号 gutter、编辑器滚动条 14/12px、`editor.fontSize` —— RdataStation 若不做代码编辑，这些不需要；数据表格区可用 12–14px 滚动条。

9. **CSS 变量生态的复杂性**（`--vscode-*` 注入、`--vscode-shadow-*`、`cornerRadius`/`bodyFontSize` token）：RdataStation 用 gpui-kit 主题结构承载语义，不需要复刻 CSS 变量层；只借鉴 "token → 变量" 的**命名规则**（`a.b.c` → 语义层级）。

### 8.4 落地建议（按收益 / 成本排序）



1. **先对齐色板**：以 Dark Modern 灰阶 + `#0078D4` 强调家族为 rds-theme 暗色基线（改动小、收益最大；`2026-dark` 差异值仅 badge/button/list 少数几个，可作注释）。

2. **引入徽标规格**：18×18 胶囊 / 3-5 内距 / 圆角 11px / 11px 字号（换算 13–14px）+ `.long` 2px 变体，写进组件规格表。

3. **登记换算表**：§8.2 的换算表作为 `ui.rs` 新增常量的取值依据（注释引用），再跑 `cargo test -p rds-workbench --test ui_contract` 校验。

4. **行为对照记录**：把 §7 的交互约定（focus 环、双击复位、hover 延迟、键盘导航）整理进 `ui-constraints.md`，与 gpui-kit 组件能力对齐，避免后续 "照抄 VS Code" 式回归。

5. **密度档位**：RD 若需要 compact 档，取 "状态栏 28 /tab 28 / 行高 24 / 控件 26" 一档（对应 VS compact 语义），不引入 22px。



***

## 9. 未确认项汇总（诚实清单）



1. `workbench.tree.indent` 配置默认值（社区传 8px，未在已抓源码定位）。

2. checkbox（Toggle Checkbox）精确宽高 CSS。

3. Dropdown / Menu 弹层单个菜单项的行高与内距（`.monaco-menu-item`）。

4. 列表 / 树 hover 延迟的具体 ms 常量（sash 的 300ms 已实证，列表不同）。

5. `editor.lineHighlightBackground` 默认色值（属 Monaco editor 内部，未在 workbench 颜色注册表定位到）。

6. Codicon 源 SVG 设计网格（16/24，两次直取 SVG 失败未实证；渲染 16px 已实证）。

7. 除活动栏（24px）与下拉箭头（16px）外，各 UI 场景图标逐控件尺寸覆盖值。

8. 面板默认高度像素、活动栏 tooltip 延迟常量。

9. `notification.*` 各 token 的 colorRegistry 注册默认值（只给了主题链实测值）。

10. `--titlebar-height` CSS 变量：core 未消费（标题栏高度由 TS 内联设置），为社区扩展约定；core 可确认的布局变量为 `--activity-bar-width/action-height/icon-size`。

11. 双击标题空白新建 tab 的绑定代码行号（约定行为已确认存在）。

12. Dark+（经典）下 `tab.active*`/`unfocused*` 精确 hex（dark\_vs 只覆盖部分；文档以 Dark Modern 为准）。



***

## 10. 完整来源清单

### O 官方文档



1. 用户界面文档（布局五段、键盘导航、tab / 面板约定）：[https://code.visualstudio.com/docs/getstarted/userinterface](https://code.visualstudio.com/docs/getstarted/userinterface)

2. 自定义布局（活动栏位置 / 尺寸、面板对齐、通知位置、pinned/locked）：[https://code.visualstudio.com/docs/configure/custom-layout](https://code.visualstudio.com/docs/configure/custom-layout)

3. 主题色 token 全量参考：[https://code.visualstudio.com/api/references/theme-color](https://code.visualstudio.com/api/references/theme-color)

4. 主题定制（colorCustomizations）：[https://code.visualstudio.com/docs/getstarted/themes](https://code.visualstudio.com/docs/getstarted/themes)

5. Codicon 图标 API：[https://code.visualstudio.com/api/references/icons](https://code.visualstudio.com/api/references/icons)

6. Codicon 预览：[https://microsoft.github.io/vscode-codicons/dist/codicon.html](https://microsoft.github.io/vscode-codicons/dist/codicon.html)

7. Release Notes v1.28（Windows 默认自定义标题栏）：[https://code.visualstudio.com/updates/v1\_28](https://code.visualstudio.com/updates/v1_28)

8. Release Notes v1.83（window.density.editorTabHeight、pinnedTabsOnSeparateRow）：[https://code.visualstudio.com/updates/v1\_83](https://code.visualstudio.com/updates/v1_83)

9. Release Notes v1.141（Compact layout density / window.density.layout）：[https://code.visualstudio.com/updates/v1\_141](https://code.visualstudio.com/updates/v1_141)

### S 源码实证（microsoft/vscode @ main，2026-10-11 经 jsdelivr / GitHub API）



* `src/vs/workbench/browser/media/style.css`（13px 根字号、字体栈、focus 1px、codicon 16px、`--vscode-*` 消费、`--window-zoom-factor`）

* `src/vs/platform/window/common/window.ts`（`DEFAULT_CUSTOM_TITLEBAR_HEIGHT = 35`）

* `src/vs/workbench/browser/parts/titlebar/media/titlebarpart.css`（窗口控制 46/16/138/70、命令中心 22/38vw/600、appicon 35/16、标题 12px、menubar min-width 36、菜单行高 30、inactive 0.6、关闭 hover 红）

* `src/vs/workbench/browser/parts/titlebar/titlebarPart.ts`（高度 35/30 逻辑、菜单栏在标题栏左区、menuBarVisibility）

* `src/vs/workbench/browser/parts/titlebar/media/menubarControl.css`（menubar token、dashed/solid outline）

* `src/vs/workbench/browser/parts/activitybar/media/activitybarpart.css`（`--activity-bar-width` 48px）

* `src/vs/workbench/browser/parts/activitybar/media/activityaction.css`（`--activity-bar-action-height` 48、`--activity-bar-icon-size` 24、活动栏徽标 16/13px、9px 字、圆角 20、menubar.compact 35px）

* `src/vs/workbench/browser/parts/statusbar/media/statusbarpart.css`（22px、12px、item 0 5px、margin 3px、hover/kind token）

* `src/vs/workbench/browser/parts/sidebar/media/sidebarpart.css`（分区头 22px 等）+ `src/vs/workbench/browser/parts/sidebar/sidebarPart.ts`（minimumWidth 170）

* `src/vs/workbench/browser/media/part.css`（`.part > .title` 35px、padding 8、h2 11px、content 13px）

* `src/vs/workbench/browser/parts/panel/media/panelpart.css`

* `src/vs/base/browser/ui/sash/sash.ts`（globalSize 4、hoverDelay 300、dblclick reset）

* `src/vs/workbench/browser/parts/editor/editorTabsControl.ts`（EDITOR\_TAB\_HEIGHT 35/22/32/28/24、`--editor-group-tab-height`）

* `src/vs/workbench/browser/parts/editor/media/editortabscontrol.css` / `multieditortabscontrol.css` / `singleeditortabscontrol.css` / `breadcrumbscontrol.css` / `editorgroupview.css`（tab 13px、宽度三档 120/80/50-160、关闭钮 28/16、sticky 38/80、面包屑、watermark 272/256/24/35）

* `src/vs/workbench/browser/parts/editor/breadcrumbsControl.ts`（SCROLLBAR\_SIZES 3/8、picker 22/36）

* `src/vs/editor/common/config/fontInfo.ts`（EDITOR\_FONT\_DEFAULTS 14/12、行高比 1.35/1.5）+ `editorOptions.ts`（minimap gutter 8/maxColumn 120、行号 minChars 5、滚动条 14/12）

* `src/vs/base/browser/ui/countBadge/countBadge.css` / `countBadge.ts`（18×18 胶囊、3/5、11px、11px 圆角、.long 变体）

* `src/vs/base/browser/ui/button/button.css`、`inputbox/inputBox.css`、`dropdown/dropdown.css`

* `src/vs/base/browser/ui/scrollbar/media/scrollbars.css`、`scrollableElementOptions.ts`（10px）、`abstractScrollbar.ts`（140px）

* `src/vs/base/browser/ui/tree/media/tree.css`（缩进 16/12、箭头槽 16、图标 10px）

* `src/vs/workbench/browser/parts/views/treeView.ts`（ITEM\_HEIGHT=22）

* `src/vs/platform/quickinput/browser/media/quickInput.css`（600px、行 22、440px、分组、计数徽章）

* `src/vs/workbench/browser/parts/notifications/media/notificationsToasts.css`、`notificationsList.css`、`notificationsToasts.ts`（450px、right 3/bottom 25、10 5、z-index）

* `src/vs/base/browser/ui/dialog/dialog.css`（480px、90vw/vh、14px/600）

* `src/vs/base/browser/ui/codicons/codicon/codicon.css`（16px/1 默认）

* `src/vs/platform/theme/common/colors/baseColors.ts` / `miscColors.ts` / `listColors.ts` / `editorColors.ts` / `minimapColors.ts` / `inputColors.ts` / `quickpickColors.ts`（registerColor 默认层）

* `extensions/theme-defaults/package.json`（10 个主题清单）+ `themes/dark_modern.json`、`light_modern.json`、`dark_vs.json`、`light_vs.json`、`dark_plus.json`、`light_plus.json`、`2026-dark.json`、`2026-light.json`（主题链与色值）

* microsoft/vscode-codicons @main README（构建 / 用法约定）

### C 社区整理



* Oppadu VS Code 设计系统快照：[https://www.oppadu.com/tools/design-systems-site/brand/vs-code.html](https://www.oppadu.com/tools/design-systems-site/brand/vs-code.html) （仅列示 / 交叉参考，未作为数值依据）

* APC customizeUI（`apc.customizeUI`）与 lehni `vscode-custom-ui-style` / `vscode-titlebar-less-macos` 扩展文档：`--titlebar-height` 等布局变量的社区用法（core 未消费，见 §2.8）

### R RdataStation 现状



* `D:\RdataStation\RDS\RdataStation-v2\.agents\skills\rds-ui-spec\SKILL.md`（ui.rs 常量表、gpui-kit 0.7.1 组件档位、间距 token、契约测试）