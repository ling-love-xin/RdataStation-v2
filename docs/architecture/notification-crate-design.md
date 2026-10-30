# `rds-notification` Crate 设计文档

## 1. 概述

### 目标
把通知系统从 `workbench` 抽出来，做成独立的通用 crate，供所有模块使用，避免依赖循环。

### 设计原则
- **轻量**：不做重 UI，只做核心逻辑 + 基础组件
- **解耦**：通知逻辑和设置分离，设置在 settings crate 里
- **可扩展**：预留通知中心、历史记录、系统通知等扩展点

---

## 2. Crate 结构

```
crates/notification/
├── Cargo.toml
├── src/
│   ├── lib.rs          # 入口，重新导出所有公共 API
│   ├── core.rs         # 核心类型：NotificationType、NotificationRecord、NotifyConfig
│   ├── service.rs      # 通知服务：推送、合并、历史存储
│   └── settings.rs    # 设置项读写（从 settings crate 分离，桥接）
```

---

## 3. 核心类型设计（`core.rs`）

### 3.1 通知类型
```rust
pub enum NotificationType {
    Info,      // 信息（蓝色）
    Success,   // 成功（绿色）
    Warning,   // 警告（黄色）
    Error,     // 错误（红色）
}
```

**方法**：
- `label()` - 中文标签
- `default_autohide()` - 默认是否自动消失
- `default_autohide_secs()` - 默认自动消失时长（秒）
- `to_gpui_type()` - 转换成 gpui-kit 的类型

### 3.2 通知记录（历史记录用）
```rust
pub struct NotificationRecord {
    pub id: String,              // 唯一 ID
    pub type_: NotificationType, // 通知类型
    pub title: String,            // 标题
    pub message: Option<String>,  // 详细内容（可选）
    pub timestamp: String,         // 推送时间（ISO 8601）
    pub read: bool,               // 是否已读
    pub delivered_to_system: bool,// 是否已推送到系统通知
}
```

### 3.3 通知配置
```rust
pub struct NotifyConfig {
    // 自动消失时长
    pub success_autohide_secs: u32,  // 成功/信息类，默认 3 秒
    pub warning_autohide_secs: u32,  // 警告类，默认 5 秒
    pub error_autohide: bool,        // 错误类是否自动消失，默认 false
    // 合并窗口
    pub merge_window_secs: u32,      // 同类通知合并窗口，默认 2 秒
    // 系统通知开关
    pub error_to_system: bool,        // 错误通知是否推送到系统，默认 true
    pub warning_to_system: bool,     // 警告通知是否推送到系统，默认 false
    pub success_to_system: bool,      // 成功通知是否推送到系统，默认 false
    pub info_to_system: bool,         // 信息通知是否推送到系统，默认 false
    // 其他
    pub do_not_disturb: bool,         // 勿扰模式，默认 false
}
```

---

## 4. 通知服务（`service.rs`）

### 4.1 对外 API

```rust
// 初始化
pub fn init_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList>;
pub fn ensure_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList>;

// 推送通知
pub fn notify(type_: NotificationType, title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App);
pub fn notify_info(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App);
pub fn notify_success(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App);
pub fn notify_warning(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App);
pub fn notify_error(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App);

// 历史记录
pub fn history(cx: &App) -> Vec<NotificationRecord>;
pub fn unread_count(cx: &App) -> usize;
pub fn mark_all_read(cx: &mut App);
pub fn clear_history(cx: &mut App);
```

### 4.2 推送流程

```
1. 读取配置（NotifyConfig）
2. 检查勿扰模式 → 是则只进历史，不弹出来
3. 检查合并窗口 → 是则合并计数
4. 构造 gpui-kit Notification
5. 根据类型设置 autohide 和 timeout
6. 根据配置决定是否推送到系统通知
7. push 到 NotificationList
8. 追加到历史记录（最多 50 条，满了删最旧的）
9. 更新未读计数
```

---

## 5. 设置桥接（`settings.rs`）

### 5.1 职责
- 从 `settings` crate 读取通知配置
- 把 `settings` crate 的类型转换成我们的 `NotifyConfig`
- 不直接依赖 settings 的内部结构，只依赖公共 API

### 5.2 为什么要这一层？
避免 `notification` crate 直接依赖 `settings` crate 的内部模型，保持解耦。

---

## 6. 迁移路径

### 第一步：创建新 crate（已完成）
- ✅ 创建 `crates/notification/` 目录
- ✅ 写 `Cargo.toml`（暂时不加入 workspace）
- ✅ 写 `src/lib.rs`、`src/core.rs`、`src/service.rs`、`src/settings.rs`

### 第二步：迁移核心代码（等你测试完再做）
- 把 `workbench/src/notification.rs` 的代码迁移到 `notification/src/service.rs`
- 把设置读取逻辑迁移到 `notification/src/settings.rs`

### 第三步：更新调用点（等你测试完再做）
```
原来：
  use crate::notification::notify_success;

改成：
  use rds_notification::notify_success;
```

### 第四步：删除旧代码（等你测试完再做）
- 删除 `workbench/src/notification.rs`
- 删除 `workbench` 对 notification 的依赖

---

## 7. 未来扩展方向

### 7.1 通知中心 UI（`view.rs`）
- 状态栏铃铛图标
- 通知中心下拉面板
- 未读计数徽标

### 7.2 通知操作按钮
```rust
// 未来可以给通知加操作按钮
notify(
    NotificationType::Error,
    "连接失败",
    Some("无法连接到 MySQL 数据库"),
    cx,
).with_action("重试", || {
    // 点击重试按钮的回调
});
```

### 7.3 通知分组
通知中心里可以按类型分组：
- 错误 (2)
- 警告 (1)
- 成功 (5)

### 7.4 通知持久化
现在只存在内存里，重启就清空。
未来可以持久化到磁盘。

---

## 8. 工作量估算

| 阶段 | 内容 | 工作量 | 状态 |
|------|------|--------|------|
| **P0** | 创建 crate + 写骨架 | 2 小时 | ✅ 已完成 |
| **P0** | 迁移核心逻辑 + 更新调用点 | 2 小时 | ⏳ 待你通知 |
| **P1** | 通知中心铃铛 + 面板 UI | 3 小时 | ⏳ 待开发 |
| **P1** | 系统通知联动 + 设置开关 | 1 小时 | ⏳ 待开发 |
| **P2** | 通知历史存储 + 未读计数 | 1 小时 | ⏳ 待开发 |
| **P2** | 勿扰模式 + 设置项 | 30 分钟 | ⏳ 待开发 |
