# RdataStation v2 · 项目进度与计划

> 维护位置：本文件（随 PR 更新）；看板：GitHub Projects（RdataStation 项目进度）。
> 更新约定：模块级设计仍以 `docs/architecture/*` 为权威；本文件只记**项目级**进度与跨模块计划。

## 1. 当前版本与发布规则

| 项 | 值 |
| --- | --- |
| 当前版本 | `0.1.0`（Cargo.toml `[workspace.package]`） |
| 发布通道 | GitHub Actions 云端构建（打标签即发布，`docs/architecture/release/release-pipeline.md`） |
| beta 标签 | `RDS-beta-<版本号>` → **仅编译 Windows x86_64** 包 |
| 定版标签 | `v<版本号>` → 恢复全平台矩阵（Linux/macOS×2） |
| 验证标签 | 验证通过后去掉 `beta` 定版 |

**已发布**：`RDS-beta-0.1.0`（2026-10-01，Windows x64 分发包 + sha256，挂于 Release Assets）。

## 2. 模块状态（主干）

| 模块 | 状态 | 备注 |
| --- | --- | --- |
| 连接（数据源/认证/网络/环境/标签分组） | ✅ M3 收尾 | 设计权威：`docs/architecture/connection/`（95 条决策） |
| 引擎（DuckDB 1.5.5 / SQLite / MySQL / PG） | ✅ 可用 | 动态链接内核，`third_party/duckdb/1.5.5/` |
| 工作台 / 编辑器 / 导航 | ✅ 可用 | 见 `docs/architecture/{editor,layout,ui}*` |
| 分析 / 洞察 / 结果集入 DuckDB | 🟡 推进中 | `docs/architecture/{analytics_resource,insight}*` |
| 插件（Extism） | 🟡 编译受限 | wasmtime 在 Windows+rustc 1.98 崩溃，未纳入 `cargo test-all` |
| gpui-kit 0.7 升级验证 | ⏳ 待验证 | 分支 `verify/gpui-kit-0.7` |

## 3. 连接模块增量计划（原型 v2.3 → 待评审）

> 原型：`docs/tmp/connection-dialog-prototype-v2.html`；评审表：`docs/tmp/connection-prototype-v2-increments-plan.md`。
> 评审通过后按 `connection-dialog-architecture.md` 的决策编号规则并入正式文档。

### #A 生命周期与留痕
- [ ] A1 连接变更历史（`connection_audit`：create/edit/sync/test 审计）
- [ ] A2 分析足迹入口（最近查询 / 结果快照 / 洞察，只做入口不复制内容）
- [ ] A3 快照同步状态徽标（仅 GP_：与全局一致 / 落后 N 次变更 / 独立）

### #B 环境策略 5+1
- [ ] ①访问级别 ②查询行数上限 ③查询超时 ④DDL 阻断 ⑤数据导出阻断（防御）
- [ ] ⑥查询留痕（强制记录 / 仅本地，对应"过程留痕可追溯"定位）
- [ ] ~~敏感列脱敏~~：用户明确**不做**

### #C 管理窗 UX
- [ ] C1 三个管理窗改 DBeaver 式 master-detail（左档案列表可搜索 + 右即选即编）
- [ ] C2 认证左栏分类（password / ssh_key / proxy_pwd / token）
- [ ] C3 网络档案认证二选一（引用配置 / 手动输入）+ 测试（单跳 + chain 逐跳探测）
- [ ] C4 chain 拓扑显示（本机客户端 → 跳板 → 目标库 · TLS）

## 4. 当前分支与 PR 约定

| 分支 | 用途 |
| --- | --- |
| `main` | 主干；发布快照在此打标签 |
| `verify/gpui-kit-0.7` | gpui-kit 0.7 升级验证（用户指定） |
| `docs/project-progress` | 本文件的维护分支（PR → main） |

约定：**改动走 PR 合入 main**（Pull Requests 页可追溯）；打标签前由 CI（`ci.yml`）做编译门禁，Release（`release.yml`）负责构建与打包。

## 5. 已知边界

- 代码签名 / 公证、安装包（MSI/AppImage/dmg）、Linux arm64 / macOS universal：明确暂不做（个人开源现状，见 release-pipeline.md §8）
- 连接模块增量计划待评审项见 §3 的待办勾选状态
