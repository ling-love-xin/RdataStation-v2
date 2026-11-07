# EDA 原型（eda）

**对应功能**：列卡展开后的 EDA 视图——迷你统计、分布条、频数排行、类型专属视图（数值/文本/布尔/日期）、折叠/展开。

**对应源码**：
- `crates/insight/src/workspace/eda.rs`（EDA 主渲染）
- `crates/insight/src/workspace/colcard/`（列卡分类型：numeric/text/boolean/datetime/folded）
- `crates/insight/src/workspace/helpers.rs`（分布条渲染）

**当前状态**：已迁移自 `insight-eda-prototype.html`（118KB）。

**已知问题**：
- [ ] 点击列卡展开时 `Duplicate a11y node id` panic（根因：`dist_bars`/`mini_freq_rows` 用显示文本拼 id）
- [ ] 列布局对齐问题（待用户描述具体现象）
