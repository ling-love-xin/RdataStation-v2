# 编辑器模块遗留问题清单（Backlog）

> 维护约定：本文档只记录**当前仍未完成 / 未接线**的事项；已完成项移入
> `editor-impl-taskbook.md` 或对应提交。更新时同步任务板（#37 等）与
> 本仓 git 提交链。状态字段：`待排期` / `已批准` / `进行中` / `下期` / `已关闭`。
> 最后盘点：2026-10-22（P1 字段注释提交后，`dbdc9f38`）。

## 一、结果网格（DBeaver 表格能力分期）

| 编号 | 项 | 内容 | 状态 |
| --- | --- | --- | --- |
| P0 | 引擎驱动上报列类型 | `QueryResult::with_batches` + 七驱动接线；`type_label` 已生效 | ✅ 已提交 `3b6e22d0` |
| P1 | 字段注释 | sqlglot 血缘映射（`projection_sources`）+ NavCache 列元数据 + 表头 tooltip 分块 | ✅ 已提交 `dbdc9f38` |
| P2 | 隐藏列 / 列统计 / BLOB / 转置 | P2-1 隐藏列（右键「隐藏此列/显示全部列」，视图列序↔数据列序折算，复制整行跳过隐藏列）+ P2-2 BLOB 摘要（`BLOB · N B`，悬停给全 hex）+ P2-3 列统计（count/NULL/min/max/avg/sum，复用值预览弹层）已提交；**P2-4 转置交互形态未定（面板级切换 vs 网格内视图、行数上限）** | ✅ 已提交（P2-1/2/3）｜P2-4 待确认设计 |
| P3 | 钻取关联数据（右键值→查关联表） | `ColumnDetail.references` 已就位，取数链路现成 | 下期（用户明确） |

P1 边界（已实现，勿当缺口）：`SELECT *` 只展开实体表；表达式列无注释；
跨 schema 同名表取首个命中；解析失败 / 未绑定连接 / Federated 全列无注释且**不影响结果**。

## 二、已讨论、方案确认但未开工

| 项 | 方案要点 | 状态 |
| --- | --- | --- |
| DBeaver 式参数绑定 | SQL 里 `:name` → 弹参数表单 → 字面替换后执行（与结果区分期无关） | 讨论过未开工 |
| LSP hover 槽位 | 评估结论：本仓高亮/补全为自研（sqlglot + 关键字表），未接 LSP | 已关闭（#21） |

## 三、历史面板（右 Dock）缺口（2026-10-22 已全部接线）

现状：`crates/editor/src/view/history.rs` + `crates/editor/src/history.rs`；
数据源 `engine::persistence::history_store`。引擎侧 `HistoryFilter` 七维
（db_type / connection_id / success_only / failed_only / start_time / end_time / keyword）
匹配逻辑齐全（`HistoryStore::filter_records`），但对外只暴露了 keyword 一维。

| 缺口 | 现状 | 接线结果 |
| --- | --- | --- |
| 过滤维度 | UI 只有关键词搜索框；`search_sql_history(keyword, limit)` 只包 keyword | 引擎加 `query_sql_history(filter, limit, offset)`（含 `to_public_record` 公共投影）；面板加成功/失败筛选（TabBar segmented）+ 库类型下拉（DropdownMenu） |
| 翻页 / 加载更多 | `PAGE = 200` 每次加载最新 200 条，无更多入口 | 面板维护 offset，满页时出「加载更多」（`load_more` 追加下一页） |
| 多选批量删除 | 只有行内单条删除 + 清空全部 | 单击行 = 勾选（`HashSet` 选中集 + `ListItem::selected`），「删除选中 (N)」按钮逐条删 |
| 右键菜单 | 无 | `ContextMenuExt`：复制 SQL（写剪贴板）/ 删除该条；双击行 = 重放（对齐 DBeaver 对象列表口径） |

> 交互口径变更：行点击原为**单击重放**，现改**单击勾选 / 双击重放 / 右键菜单**（为多选让路）。
> 测试：`view::history` 7 例 + `history::tests` 7 例全绿（新增成败筛选 / 库类型筛选 / 多选删除 / 加载更多）。

## 四、结果网格其它

| 项 | 现状 | 说明 |
| --- | --- | --- |
| 列宽持久化 | DataTable/TableState 列宽拖拽现成，但不持久化 | 换文档/重启恢复默认宽；待评估是否落 NavCache/会话 |
| analyze 路径 | 分析/引擎内结果无列类型也无注释 | `ResultSet` 无类型字段；P1 已按「无注释」如实处理（已知边界） |

## 五、工程与流程

| 项 | 现状 | 影响 / 处置 |
| --- | --- | --- |
| origin/main 未推送 | 本地领先 14 个 commit（0.8.0 升级、P0、P1、多文档恢复、值查看器等） | 等用户拍板推送 |
| insight 线大量未提交 | 工作区 47 处：`crates/insight/*`、`plugin/*`、`workbench/tests/*`（3 个）、docs/insight 等 | 阻塞 `cargo test -p rds-workbench` 整包（test-cfg 编译错误）；P 线提交不碰 Cargo.lock / insight 文件 |
| Cargo.lock 未提交改动 | 被其他线改过 | 不随 P 线提交 |

## 六、已完结（盘点确认无遗留）

高亮修复（sql/python/rust/rlang/log）→ 分析模式（内核接入 / 渲染分发 / cell 升级 /
串行队列 / 笔记级动作）→ CSV 表格 → 多文档恢复 → 补全缓存回落实时内省 →
值查看器 → 结果区高度持久化 → P0 列类型 → P1 字段注释 → 工具栏/状态栏
对齐 + 图标化 + 微标拆分 + 行号槽收窄 + 左裁修复 → 通用/SQL 模式对齐核对 →
历史面板缺口（过滤 / 加载更多 / 多选删除 / 右键菜单，2026-10-22）→
P2 结果网格增强之隐藏列 / BLOB 摘要 / 列统计（2026-10-22，P2-4 转置待确认设计）。

## 建议优先级

1. P2-4 转置（交互形态待用户确认后动工）
2. 参数绑定（方案已确认）
3. P3 钻取（下期）
