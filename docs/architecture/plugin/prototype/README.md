# 插件系统（M9）· 可交互原型

> 这些是**评审用的形态稿，不是产品代码**。双击即开：无网络、无构建、无 CDN。
> 形态与决策的设计出处见 `../plugin-prototype-design.md`；任务与排期见 `../plugin-dev-plan.md`。

## 五张稿，覆盖每一种承载

| 文件 | 承载 | 这一稿要回答的问题 |
| --- | --- | --- |
| `plugin-entry.html` | **宿主自身 UI** | 「插件」入口 / 安装 / 启用 / 授权 / 详情长什么样。管理界面**必须由宿主渲染**，否则插件能把自己伪装成宿主 |
| `host-rendered-panel.html` | **宿主渲染的面板**（VS Code 模型） | 插件不画 UI 时收益具体长什么样：树 / 菜单 / 进度与取消 / 空态 / 未安装占位 / 崩溃隔离 |
| `webview-chart-panel.html` | **webview · 档① 全屏面板** | 面板只画不聚合（重算回宿主/DuckDB）；能力未授予回 `-32006`；主题跟宿主走 |
| `dashboard-window.html` | **webview · 档② 独立窗口** | 大结果集怎么进 webview（Arrow 分块，不是几 MB JSON）；取消与窗口销毁 |
| `driver-plugin.html` | **驱动 sidecar（native）** | 三轨来源 / 能力声明 / 进程健康与重启 / native 权限与凭据口径 |

档③（与 GPUI 元素混排）**故意没有稿** —— Q8 已判定实践上不支持（原生子窗口的层级不受 GPUI 控制），做稿会给人「这能用」的错觉。

## 共同约定（五张稿都遵守）

- **单文件自包含**：不用 CDN、不拉图标字体、不依赖构建。理由：评审时可能离线，且"打开就能看"比"像真的"更重要。
- **色值取自本仓真实资产**：`assets/themes/rds-theme.json`（RDS Dark / RDS Light）+ `assets/themes/product-tokens.json`。稿子里没有我随手挑的颜色。
- **尺寸取自 `crates/workbench_shell/src/ui.rs`**：标题栏 2.25rem、活动栏 3rem、左 Dock 15rem、面板头 2.25rem、列表行 1.5rem、小控件 1.625rem、细线 1px。状态栏高度由 gpui-kit `StatusBar` 组件决定，稿里只为比例接近。
- **文件头写明"与真实现的差异"**：避免有人拿稿子当规格。规格永远是 `../plugin-prototype-design.md` 与 `../ui/`、`../theme/`。
- **底部有「本稿演示了什么」**：逐条对应设计文档的哪一节；也写清**刻意没画**的部分（市场评分、更新策略等未拍板项）。
- **宿主侧界面（GPUI）与插件侧（webview）用不同框架画出**：`bounds-tag` / 「宿主模拟器」就是那条边界的可视化，避免把两者混为一谈。

## 自查（可选，需要 node ≥ 18）

```sh
cd docs/architecture/plugin/prototype
node check-prototypes.mjs
```

它做三件事：① 抽出 `<script>` 做语法解析；② 用最小 DOM 桩把每个原型**真跑一遍**（抓"启动即抛"）；③ 按每个原型登记好的**交互探针**点几下（抓"点了没反应"）。它**不**验证外观——视觉只能人眼过。

## 维护约定

1. 决策变了 → 同步**对应的那一张稿**，并更新本文件的两张表。
2. 新增原型 → 必须在 `check-prototypes.mjs` 的 `PROBES` 里登记一条探针（没登记会被判失败）。
3. **不要在这里画装饰性假图**：每张稿都必须能回答上面表里那个问题，否则它只是噪音。
4. 稿子与文档冲突时：**以文档为准**，然后回来改稿。
