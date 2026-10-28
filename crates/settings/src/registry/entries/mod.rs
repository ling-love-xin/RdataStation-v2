//! 设置登记数据：按域拆分，`all()` 合并成全局登记表（`registry::registry()` 惰性缓存）。
//!
//! 新增一个域 = 这里加 `pub mod` + 各域文件写 `pub fn entries()` + `all()` 里 `extend`。
//! 域顺序 = 登记顺序（`page_groups` / `sections` 契约测试依赖它，勿乱调）。
use super::SettingSpec;

pub mod appearance;
pub mod editor;
pub mod navigator;
pub mod resources;
pub mod connection;
pub mod projects;
pub mod logging;
pub mod startup;
pub mod scratchpad;
pub mod insight;
pub mod result_panel;
pub mod layout;
pub mod notifications;

/// 合并全部登记项（固定域序）。
pub fn all() -> Vec<SettingSpec> {
    let mut v = Vec::new();
    v.extend(appearance::entries());
    v.extend(editor::entries());
    v.extend(navigator::entries());
    v.extend(resources::entries());
    v.extend(connection::entries());
    v.extend(projects::entries());
    v.extend(logging::entries());
    v.extend(startup::entries());
    v.extend(scratchpad::entries());
    v.extend(insight::entries());
    v.extend(result_panel::entries());
    v.extend(layout::entries());
    v.extend(notifications::entries());
    v
}