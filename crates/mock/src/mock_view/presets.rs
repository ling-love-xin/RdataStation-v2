// ==================== 字段预设 ====================

// 继承父模块（mock_view）的全部 use 绑定：std / gpui_kit / models / persistence / generator_catalog 等
use super::*;

/// 字段预设：常用业务字段的一行打包（列名 + 类型 + 生成器 + 空值率 + 唯一 + 置信度）。
///
/// 与三种既有粒度的关系（设计决策见 `docs/architecture/mock/mock-view-ui-design.md` §8.3）：
/// - generator_catalog：只有生成器，不含类型 / 空值率 / 唯一打包——预设在这里打包；
/// - schema_map 列名映射：代码内建规则只推生成器——预设是「常用列」的可视入口；
/// - 场景模板 / 用户模板：整表粒度——预设是单字段粒度。
///
/// 预设是**添加动作的快捷方式**：应用后生成普通列（列名重名时自动加序号），
/// 之后照常走列编辑对话框微调，不锁定任何后续编辑。
pub struct FieldPreset {
    /// 稳定标识（snake_case）
    pub id: &'static str,
    /// 菜单显示名（如「姓名」「电话」）
    pub label: &'static str,
    /// 分组（菜单按组分隔）
    pub category: PresetCategory,
    /// 打包的列定义
    pub column: ColumnDef,
    /// 应用时的置信度（预设即推荐 → `high`）
    pub confidence: &'static str,
}

/// 预设分组（菜单顺序即 `ALL` 顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetCategory {
    Common,
    Person,
    Address,
    Business,
    Tech,
}

impl PresetCategory {
    pub const ALL: [PresetCategory; 5] = [
        PresetCategory::Common,
        PresetCategory::Person,
        PresetCategory::Address,
        PresetCategory::Business,
        PresetCategory::Tech,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            PresetCategory::Common => "常用",
            PresetCategory::Person => "人物",
            PresetCategory::Address => "地址",
            PresetCategory::Business => "业务",
            PresetCategory::Tech => "技术",
        }
    }
}

/// 内建预设全集（按分组顺序）。全部由 generator_catalog 已有生成器打包（实证存在：
/// Name/PhoneNumber/Email/Username/City/StreetName/ZipCode/CompanyName/JobTitle/
/// DateTime/Boolean/RandomDecimal/UuidV4），不引入新生成器。
pub fn builtin_presets() -> Vec<FieldPreset> {
    vec![
        // 常用
        FieldPreset {
            id: "date",
            label: "日期",
            category: PresetCategory::Common,
            confidence: "high",
            column: ColumnDef {
                name: "created_at".into(),
                data_type: ColumnDataType::Date,
                generator: GeneratorConfig::DateTime {
                    min: "2020-01-01".into(),
                    max: "2030-12-31".into(),
                    workdays_only: false,
                    work_hours_only: false,
                    work_hour_start: "09:00".into(),
                    work_hour_end: "18:00".into(),
                    work_week: "1111111".into(),
                    skip_dates: vec![],
                    work_dates: vec![],
                },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "boolean",
            label: "布尔值",
            category: PresetCategory::Common,
            confidence: "high",
            column: ColumnDef {
                name: "is_active".into(),
                data_type: ColumnDataType::Boolean,
                generator: GeneratorConfig::Boolean { ratio: 50 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        },
        // 人物
        FieldPreset {
            id: "name",
            label: "姓名",
            category: PresetCategory::Person,
            confidence: "high",
            column: ColumnDef {
                name: "name".into(),
                data_type: ColumnDataType::Varchar { length: Some(50) },
                generator: GeneratorConfig::Name,
                nullable_ratio: 0.05,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "phone",
            label: "电话",
            category: PresetCategory::Person,
            confidence: "high",
            column: ColumnDef {
                name: "phone".into(),
                data_type: ColumnDataType::Varchar { length: Some(30) },
                generator: GeneratorConfig::PhoneNumber,
                nullable_ratio: 0.1,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "email",
            label: "邮箱",
            category: PresetCategory::Person,
            confidence: "high",
            column: ColumnDef {
                name: "email".into(),
                data_type: ColumnDataType::Varchar { length: Some(100) },
                generator: GeneratorConfig::Email,
                nullable_ratio: 0.05,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "username",
            label: "用户名",
            category: PresetCategory::Person,
            confidence: "high",
            column: ColumnDef {
                name: "username".into(),
                data_type: ColumnDataType::Varchar { length: Some(50) },
                generator: GeneratorConfig::Username,
                nullable_ratio: 0.0,
                unique: true,
                dependency: None,
            },
        },
        // 地址
        FieldPreset {
            id: "city",
            label: "城市",
            category: PresetCategory::Address,
            confidence: "high",
            column: ColumnDef {
                name: "city".into(),
                data_type: ColumnDataType::Varchar { length: Some(50) },
                generator: GeneratorConfig::City,
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "street",
            label: "街道",
            category: PresetCategory::Address,
            confidence: "high",
            column: ColumnDef {
                name: "street".into(),
                data_type: ColumnDataType::Varchar { length: Some(100) },
                generator: GeneratorConfig::StreetName,
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "zip",
            label: "邮编",
            category: PresetCategory::Address,
            confidence: "high",
            column: ColumnDef {
                name: "zip".into(),
                data_type: ColumnDataType::Varchar { length: Some(20) },
                generator: GeneratorConfig::ZipCode,
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        },
        // 业务
        FieldPreset {
            id: "company",
            label: "公司",
            category: PresetCategory::Business,
            confidence: "high",
            column: ColumnDef {
                name: "company".into(),
                data_type: ColumnDataType::Varchar { length: Some(100) },
                generator: GeneratorConfig::CompanyName,
                nullable_ratio: 0.05,
                unique: false,
                dependency: None,
            },
        },
        FieldPreset {
            id: "price",
            label: "金额",
            category: PresetCategory::Business,
            confidence: "high",
            column: ColumnDef {
                name: "price".into(),
                data_type: ColumnDataType::Decimal {
                    precision: 10,
                    scale: 2,
                },
                generator: GeneratorConfig::RandomDecimal {
                    min: 1.0,
                    max: 100_000.0,
                    scale: 2,
                },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        },
        // 技术
        FieldPreset {
            id: "uuid",
            label: "UUID",
            category: PresetCategory::Tech,
            confidence: "high",
            column: ColumnDef {
                name: "id".into(),
                data_type: ColumnDataType::Uuid,
                generator: GeneratorConfig::UuidV4,
                nullable_ratio: 0.0,
                unique: true,
                dependency: None,
            },
        },
    ]
}

/// 找预设（按稳定 id）。菜单闭包里按 id 找回来，避免把 `&FieldPreset` 直接搬进闭包。
pub fn preset_by_id(id: &str) -> Option<FieldPreset> {
    builtin_presets().into_iter().find(|p| p.id == id)
}
