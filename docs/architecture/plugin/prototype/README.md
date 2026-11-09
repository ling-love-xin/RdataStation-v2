# 插件系统（M9）· 可交互原型

> 这些是**评审用的形态稿，不是产品代码**。双击即开：无网络、无构建、无 CDN。
> 六张稿里，`plugin-manager.html` 是「入口 + 同名目录资源」；其余五张仍是单文件。
> 形态与决策的设计出处见 `../plugin-prototype-design.md`；任务与排期见 `../plugin-dev-plan.md`。

## 六张稿，覆盖每一种承载

| 文件 | 承载 | 这一稿要回答的问题 |
| --- | --- | --- |
| `plugin-manager.html` | **宿主自身 UI · 管理页**（入口页；样式 / 脚本在同名目录 `plugin-manager/`） | 插件管理从 240px Dock 升级为中央页后长什么样。参考 Zed `extensions_ui`：搜索 + 状态筛选 + 贡献点分类 chips + 卡片列表；右侧固定详情承载贡献点 / 权限 / 项目引用 / 运行状态。**与 `plugin-entry.html` 的边界**：本稿只画「已安装插件的管理」，安装入口、授权流程与单插件详情仍以 `plugin-entry.html` 为准 |
| `plugin-entry.html` | **宿主自身 UI** | 「插件」入口 / 安装 / 启用 / 授权 / 详情长什么样。管理界面**必须由宿主渲染**，否则插件能把自己伪装成宿主 |
| `host-rendered-panel.html` | **宿主渲染的面板**（VS Code 模型） | 插件不画 UI 时收益具体长什么样：树 / 菜单 / 进度与取消 / 空态 / 未安装占位 / 崩溃隔离 |
| `webview-chart-panel.html` | **webview · 档① 全屏面板** | 面板只画不聚合（重算回宿主/DuckDB）；能力未授予回 `-32006`；主题跟宿主走 |
| `dashboard-window.html` | **webview · 档② 独立窗口** | 大结果集怎么进 webview（Arrow 分块，不是几 MB JSON）；取消与窗口销毁 |
| `driver-plugin.html` | **驱动 sidecar（native）** | 三轨来源 / 能力声明 / 进程健康与重启 / native 权限与凭据口径 |

档③（与 GPUI 元素混排）**故意没有稿** —— Q8 已判定实践上不支持（原生子窗口的层级不受 GPUI 控制），做稿会给人「这能用」的错觉。

## 共同约定（六张稿都遵守）

- **每张稿自包含，但可以分目录**：不依赖 CDN、图标字体、构建或网络。简单稿保持单文件；复杂稿用「语义化入口 `.html` + 同名目录」：样式进 `styles/`、脚本进 `scripts/`，入口只留 DOM 骨架与相对引用。双击入口即开；`file://` 下不 `fetch`、不用 `type="module"`。理由：评审时可能离线，且"打开就能看"比"像真的"更重要。
- **色值取自本仓真实资产**：`assets/themes/rds-theme.json`（RDS Dark / RDS Light）+ `assets/themes/product-tokens.json`。稿子里没有我随手挑的颜色。
- **尺寸取自 `crates/workbench_shell/src/ui.rs`**：标题栏 2.25rem、活动栏 3rem、左 Dock 15rem、面板头 2.25rem、列表行 1.5rem、小控件 1.625rem、细线 1px。状态栏高度由 gpui-kit `StatusBar` 组件决定，稿里只为比例接近。
- **文件头写明"与真实现的差异"**：避免有人拿稿子当规格。规格永远是 `../plugin-prototype-design.md` 与 `../ui/`、`../theme/`。
- **底部有「本稿演示了什么」**：逐条对应设计文档的哪一节；也写清**刻意没画**的部分（市场评分、更新策略等未拍板项）。
- **宿主侧界面（GPUI）与插件侧（webview）用不同框架画出**：`bounds-tag` / 「宿主模拟器」就是那条边界的可视化，避免把两者混为一谈。

## 目录结构

```text
prototype/
├─ README.md
├─ check-prototypes.mjs
├─ plugin-manager.html                 # 入口：DOM 骨架 + 相对 link / script
├─ plugin-manager/
│  ├─ styles/
│  │  ├─ tokens.css                    # 明暗 token + reset
│  │  ├─ shell.css                     # 评审条 / 标题栏 / 活动栏 / Dock / 状态栏 / 底部说明
│  │  ├─ page.css                      # 中央管理页 / 统计 / 工具栏 / 卡片
│  │  ├─ detail.css                    # 右侧详情
│  │  └─ overlays.css                  # 弹层 / Toast
│  └─ scripts/
│     ├─ data.js                       # 8 条种子数据 + state
│     ├─ helpers.js                    # 转义 / 状态 / 计数 / 筛选
│     ├─ view-list.js                  # 统计 / 筛选 / 卡片
│     ├─ view-detail.js                # 右侧详情
│     ├─ view-dock.js                  # 左 Dock + render
│     ├─ actions.js                    # 启停 / 升级 / 重试 / 引用
│     ├─ dialogs.js                    # 安装 / 卸载弹层
│     ├─ events.js                     # 事件委托 / 搜索 / 排序 / 主题
│     └─ bootstrap.js                  # 启动
├─ plugin-entry.html                   # 以下五张保持单文件
├─ host-rendered-panel.html
├─ webview-chart-panel.html
├─ dashboard-window.html
└─ driver-plugin.html
```

**拆分门槛**：单文件超过约 40KB，或已出现第三个独立视图（列表 / 详情 / 弹层）时，按上述结构拆；不要提前拆小稿。公共 token 先在 `tokens.css`；等第二张目录稿出现，再抽到 `shared/`，避免过早抽象。新增目录稿时，入口文件名仍是 `PROBES` 的 key，不要改成 `index.html`。

## 自查（可选，需要 node ≥ 18）

```sh
cd docs/architecture/plugin/prototype
node check-prototypes.mjs
```

它做三件事：① 抽出内联或同目录外链 `<script>` 做语法解析；② 用最小 DOM 桩把每个原型**真跑一遍**（抓"启动即抛"）；③ 按每个原型登记好的**交互探针**点几下（抓"点了没反应"）。它**不**验证外观——视觉只能人眼过。

## 维护约定

1. 决策变了 → 同步**对应的那一张稿**，并更新本文件的两张表。
2. 新增原型 → 必须在 `check-prototypes.mjs` 的 `PROBES` 里登记一条探针（没登记会被判失败）；目录稿用入口文件名登记。
3. **不要在这里画装饰性假图**：每张稿都必须能回答上面表里那个问题，否则它只是噪音。
4. 稿子与文档冲突时：**以文档为准**，然后回来改稿。
