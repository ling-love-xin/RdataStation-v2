# DuckDB 2.0 与 GPUI-kit 0.8.1 升级评估

- 调研日期：2026-10-27
- 评估对象：DuckDB `2.0.0`（duckdb-rs `1.20000.0`）、gpui-kit `0.8.1`
- 项目基线：`duckdb 1.10505.0`（DuckDB v1.5.5，动态链接）、`gpui-kit 0.8.0`（vendor 私有 patch）
- 证据来源：crates.io 元数据 / duckdb.org 官方发布公告 / 本地 0.8.1 与 0.8.0 源码逐文件 diff（非在线文档推断）

## 1. 版本存在性与时间线

| 项 | 版本 | 发布时间 | 说明 |
| --- | --- | --- | --- |
| DuckDB | 2.0.0 LTS "Cinnamon Teal" | 2026-10-21 | 官方最大 feature release，支持至 2027-10 |
| duckdb-rs | 1.20000.0 | 2026-10-22 | 版本编码 `1.MAJOR_MINOR_PATCH.x` → DuckDB 2.0.0；发布仅 5 天 |
| gpui-kit | 0.8.1 | 2026-10-26 | 当前最新版（default/max_stable），0.8.0 发布于 10-19 |
| gpui-base/gpui-component/gpui-kit-assets | 0.8.1 | — | gpui-kit 0.8.1 依赖 `^0.8.1` |
| gpui-pre | =0.3.10 | — | 0.8.0 用 0.3.9 → 0.8.1 升 0.3.10 |

> 注：支持线——DuckDB 1.4 支持至 2026-11-17；**1.5 已延期至 2027-03（同样转为 LTS）**。项目当前 1.5.5 无紧迫升级压力。

## 2. GPUI-kit 0.8.0 → 0.8.1（建议：可跟升）

### 变更性质：符号兼容的 patch 增量

- 对 0.8.1 与 0.8.0 原始源码逐文件 diff（排除项目 patch 干扰）：
  - gpui-base：**+4 文件 / 84 文件变更 / 0 删除**
  - gpui-component：**+0 文件 / 112 文件变更 / 0 删除**
- 项目消费的核心组件（button / input / tab / list / select / combobox / menu / table / dock / tooltip / dialog / theme）：**0.8.0→0.8.1 无任何被删除的 `pub fn / struct / enum / trait / const / use`** —— 项目现有消费点无 API 硬断。

### 实质新能力 / 修复（本地源码实证）

| 项 | 位置 | 对项目价值 |
| --- | --- | --- |
| 编辑器 Tab 制表宽度统一 | gpui-base `input/base/tab_stops.rs`（新） | DirectWrite / CoreText 对 `\t` 宽度不一致 → 跨平台编辑器缩进一致，SQL 编辑器直接受益 |
| a11y 无障碍 | `input/base/state.rs` `InputAccessibility`（新） | 桌面工具可访问性增强 |
| 编辑器自定义滚动条 | `input/scrollbar_presentation.rs`（新）、`EditorScrollbarRenderer`、`scrollbar_renderer` 字段 | 结果集 / 编辑区滚动条定制 |
| 平台化命令键 | `cmd-enter` / `ctrl-enter` | 快捷键平台适配 |
| gpui-pre 0.3.9→0.3.10 | `ScrollWheelEvent` → `ScrollEvent` | 项目代码**不直接使用**这两个符号，无波及 |

### 升级成本与风险

- vendor 换 0.8.1 源码，重打 6 处私有 patch（gpui-base `state.rs` 5 项 + gpui-component `dock/mod.rs` DockSkin 配置）。
- 实证：0.8.1 原始源码**不含**全部私有 patch 符号（`add_cursor_above_line` / `collapse_cursors` / `cursor_count` / `select_columnar_block` / `tab_bar_max_width` / `tab_bar_variant` 均 MISS），但依赖基础（`ColumnarPoint` / `build_columnar_selection` / `SkinShared`）保留；**`dock/mod.rs` 纯 upstream 0.8.0→0.8.1 零变化**，patch 上下文稳定 → 重打成本低。
- 风险：低。需按旧流程跑 `cargo tree -d` 确认 patch 命中、`-j 2` 全量测试。

## 3. DuckDB 1.5.5 → 2.0.0（建议：暂缓）

### 新特性（对项目潜在价值）

| 特性 | 说明 | 项目关联度 |
| --- | --- | --- |
| Server 模式（Quack）+ `CONNECT` | 传统 client-server，远程 pushdown optimizer | 中低——桌面内嵌引擎场景价值有限；若未来做"共享分析服务"或 DuckDB 连接类型才显价值 |
| 异步 I/O | Parquet / CSV / 原生存储读放大并行 | 中——数据分析 / 文件读取提速 |
| `VARIANT` 一等公民 | 半结构化 schema-less 数据列式执行 | 中——EDA / insight 处理 JSON 提速 |
| Triggers | 审计表、物化视图增量维护（含 transition tables） | 中——二次分析落库数据治理 |
| 新 PEG SQL parser（默认） | 扩展可挂接语法，错误带精确定位 | 低——项目 SQL 解析/转译走 `sqlglot-rust`，不直接用 DuckDB parser |
| 嵌套 schema / NEAREST joins / JSON mutation / 数据修改 CTE | SQL 方言增强 | 中低——向量 / embedding 相似度 join 对 insight 有一定价值 |

### 升级成本（硬性）

1. **arrow 58.4.0 → ^60**：duckdb-rs `1.20000.0` 依赖 `arrow ^60`；项目根 `Cargo.toml` arrow 由 duckdb 决定，**必须连带升级**（主版本跳 2，需核对项目 query_arrow / vtab-arrow / 结果集 RecordBatch 的 arrow 用法）。
2. **动态库 1.5.5 → 2.0.0**：`third_party/duckdb/1.5.5`（40MB 含 lib/dll）需替换为 2.0.0，`DUCKDB_LIB_DIR` 同步，`duckdb.dll` 部署复制。
3. **C API v2**：libduckdb-sys `=1.20000.0` 走 `capi-v1` feature；内核 API 层变化需验证 FFI 兼容。
4. **Rust 工具链**：`1.20000.0` 要求 `rust 1.88`。

### 风险（高）

- **存储格式破坏性**：DuckDB 2.0 写入的新格式，**2.0 之前的版本无法读取**；2.0 可读旧库。项目分析模式 / 双实例的存量 `.duckdb` 文件需评估。
- **生态过新**：duckdb-rs `1.20000.0` 发布仅 5 天；官方建议 mission-critical 等 11 月首个 patch，先在 shadow 系统测试。
- 版本匹配要求高：crate 版本、动态库版本、arrow 版本三者必须成对，任何错配编译即断。

## 4. 结论与建议

- **GPUI-kit 0.8.1：建议随下次 UI / 编辑器改动一并升级**。低成本（符号兼容、patch 可重打）、确定性收益（7 天内的 bugfix 累积 + 编辑器 Tab 宽度 / a11y / 自定义滚动条），非急迫但顺手。
- **DuckDB 2.0.0：暂缓**。收益以数据分析能力增强为主、非当前核心刚需；成本（arrow 58→60 连带、动态库、C API v2）与风险（存储格式破坏性、生态仅 5 天）偏高。**等待 11 月首个 patch + duckdb-rs 生态稳定后，以 shadow 分支单独验证再评估**；当前 1.5 线已延期为 LTS（至 2027-03），无紧迫升级压力。

## 5. 待办 / 缺口

- [ ] gpui-kit 0.8.1：在 worktree 重打 vendor patch 后跑 `cargo build --workspace` + `cargo test --workspace -j 2` 验证。
- [ ] 若升 0.8.1，复核 gpui-pre 0.3.10 下 editor 相关（LSP / text）行为回归（SKILL 提醒：patch 静默弃用风险）。
- [ ] DuckDB 2.0 评估：跟踪 duckdb-rs 1.20000.x 首个 patch；记录项目内 arrow 58 API 使用点清单（升级前核对）。
