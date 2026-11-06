//! 排序键与升降序：标签、设置键、缺值处理。

use super::*;

/// 排序字段（原型 §2.2 的五项）。
///
/// 比的是**行上的原始值**（时间戳 / 字节数），不是格式化后的尾巴字符串——
/// 拿 `1.2 KB` 与 `900 B` 比较会静默排错（前者小于后者的字节数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    Name,
    ArchivedAt,
    UpdatedAt,
    Size,
    Version,
}

impl SortField {
    /// 菜单里的顺序（原型 §2.2 的“名称 / 归档时间 / 更新时间 / 大小 / 版本号”）。
    pub const ALL: [SortField; 5] = [
        Self::Name,
        Self::ArchivedAt,
        Self::UpdatedAt,
        Self::Size,
        Self::Version,
    ];

    /// 菜单文案（与 `ui.rs` 的尺寸常量为同一类"就近常量"）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::ArchivedAt => "归档时间",
            Self::UpdatedAt => "更新时间",
            Self::Size => "大小",
            Self::Version => "版本号",
        }
    }

    /// 落盘 key（设置项 `resources.default_sort` 的取值；与菜单文案分家——文案会改，key 不会）。
    pub fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::ArchivedAt => "archived_at",
            Self::UpdatedAt => "updated_at",
            Self::Size => "size",
            Self::Version => "version",
        }
    }

    /// 解析落盘 key；未知值返回 `None`（**不猜**，由调用方回退到默认）。
    pub fn from_key(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.key() == text)
    }

    /// 该字段的默认方向：名称升序（A→Z 是直觉），时间 / 大小 / 版本**降序**
    /// （用户点这几列想看的是"最新 / 最大"，不是"最早 / 最小"）。
    pub fn default_order(self) -> SortOrder {
        match self {
            Self::Name => SortOrder::Asc,
            Self::ArchivedAt | Self::UpdatedAt | Self::Size | Self::Version => SortOrder::Desc,
        }
    }

    /// 工具栏按钮上的短文案。
    ///
    /// 按钮要跟着搜索框抢 240px 面板的宽度：四个字的「归档时间」会把搜索框挤到没法用
    /// （菜单里有全名，不靠按钮认字段）。
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::ArchivedAt => "归档",
            Self::UpdatedAt => "更新",
            Self::Size => "大小",
            Self::Version => "版本",
        }
    }
}

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    /// 同一字段再点一次即翻转（沿用 M4 与 v1 `use-pagination` 的语义）。
    pub fn flipped(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    /// 方向箭头（排序菜单项与工具栏按钮共用，见 `SortField::label` 的同类注释）。
    pub fn arrow(self) -> &'static str {
        match self {
            Self::Asc => "↑",
            Self::Desc => "↓",
        }
    }
}
