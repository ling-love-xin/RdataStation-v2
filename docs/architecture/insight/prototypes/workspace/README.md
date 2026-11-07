# 工作区原型（workspace）

**对应功能**：洞察工作区主框架——顶部 tab 条、左栏（对象树/操作）、右栏（EDA/总览/设置）、tab 切换、多对象并列。

**对应源码**：
- `crates/insight/src/workspace/mod.rs`（面板主装配）
- `crates/insight/src/workspace/toolbar.rs`（顶部工具条）

**当前状态**：已迁移自 `insight-workspace-prototype.html`（77KB），A 方案多标签布局已落地。

**待改点**：
- [ ] 表名左对齐（当前居中，应与微标一致左对齐）
- [ ] 滚动条样式对齐
- [ ] 步骤卡位置确认
