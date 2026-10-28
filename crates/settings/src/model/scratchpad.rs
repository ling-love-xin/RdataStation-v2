//! 草稿箱设置 model（struct + 默认值 + 默认函数）。
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// 与面板 `ToolbarButtonKey::side()` 的默认口径一致——只记录用户显式改过的键。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scratchpad {
    /// 工具栏按钮配置：键 = 按钮稳定标识（`search`/`new_file`/`new_folder`/
    /// `reference`/`import`/`sort`/`refresh`）；值 = 显示/隐藏 + 左/右位置。
    #[serde(default)]
    pub toolbar: BTreeMap<String, ToolbarButtonCfg>,
    /// 草稿/引用/回收站可拖拽分区的最大高度（rem，默认 7.5 = 面板默认，
    /// 区间 [2.0, 18.0]，2026-10-28 B3 接线持久化；数值形态方便设置契约）。
    #[serde(default)]
    pub group_max_h: f32,
}

impl Default for Scratchpad {
    fn default() -> Self {
        Self {
            toolbar: BTreeMap::new(),
            group_max_h: 7.5,
        }
    }
}

/// 单个工具栏按钮的配置（⋯ 菜单写入；落盘 `{ "visible": bool, "side": "left"|"right" }`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolbarButtonCfg {
    /// 是否在工具栏显示（默认显示）。
    #[serde(default = "default_toolbar_visible")]
    pub visible: bool,
    /// 所在侧（默认 `left`；`right` 显式写才用——面板按按钮默认侧兜底，
    /// 因此空配置与「未写 side」都回到原型默认左 5 右 2）。
    #[serde(default = "default_toolbar_side")]
    pub side: String,
    /// 工具栏显示顺序（⋯ 菜单 ↑↓ 调整；未写/负值 = 原型默认顺序（ALL 枚举序））。
    #[serde(default = "default_toolbar_order")]
    pub order: i32,
}

impl Default for ToolbarButtonCfg {
    fn default() -> Self {
        Self {
            visible: default_toolbar_visible(),
            side: default_toolbar_side(),
            order: default_toolbar_order(),
        }
    }
}

fn default_toolbar_order() -> i32 {
    -1
}

fn default_toolbar_visible() -> bool {
    true
}

fn default_toolbar_side() -> String {
    "left".to_string()
}
