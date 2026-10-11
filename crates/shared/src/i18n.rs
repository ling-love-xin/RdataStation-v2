//! 界面国际化：中英双语资源层。
//!
//! 约定（design-guides §Internationalization）：
//! - **中文原文是 key**（项目界面语言，stable intent；改动最小——字符串就地包 `tr(...)`）；
//! - 英文为翻译；`tr()` 按当前 [`Locale`] 查表，无翻译项回退中文原文；
//! - 写每个语言，不翻译其形状：英文词序 / 句法独立定，不逐字对译；
//! - 复用的同一词（如“保存”）全仓同译，保持产品词汇表一致。
//!
//! Locale 全局切换（`set_locale`）由设置页 `appearance.locale` 的 effect 驱动；
//! 不依赖 gpui（本 crate 是纯 Rust 基础层），跨线程安全。

/// 界面语言。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Locale {
    /// 简体中文（默认）
    #[default]
    Zh,
    /// 英语
    En,
}

impl Locale {
    /// 设置存储码（`appearance.locale` 取值）。
    pub fn code(self) -> &'static str {
        match self {
            Locale::Zh => "zh",
            Locale::En => "en",
        }
    }

    /// 从设置存储码解析（未知值回退中文）。
    pub fn from_code(s: &str) -> Locale {
        match s {
            "en" => Locale::En,
            _ => Locale::Zh,
        }
    }
}

use std::sync::atomic::{AtomicU8, Ordering};

/// 当前语言（0 = Zh，1 = En）。
static LOCALE: AtomicU8 = AtomicU8::new(0);

/// 设置当前语言（设置页 `appearance.locale` effect 调用）。
pub fn set_locale(l: Locale) {
    LOCALE.store(l as u8, Ordering::Relaxed);
}

/// 当前语言。
pub fn current() -> Locale {
    match LOCALE.load(Ordering::Relaxed) {
        1 => Locale::En,
        _ => Locale::Zh,
    }
}

/// 中文原文 → 英文。只登记界面文案；数据 / 错误消息按需补。
/// 保持有序列表，新增翻译追加到末尾。
static EN: &[(&str, &str)] = &[
    // 外壳 / 面板
    ("数据源", "Data Sources"),
    ("草稿箱", "Drafts"),
    ("资产库", "Assets"),
    ("Mock", "Mock"),
    ("搜索", "Search"),
    ("状态", "Status"),
    ("设置", "Settings"),
    // 连接对话框
    ("连接", "Connection"),
    ("新建连接", "New Connection"),
    ("编辑连接", "Edit Connection"),
    ("保存", "Save"),
    ("保存并关闭", "Save & Close"),
    ("取消", "Cancel"),
    ("测试连接", "Test Connection"),
    ("删除", "Delete"),
    ("重命名", "Rename"),
    ("刷新元数据", "Refresh Metadata"),
    ("断开当前连接", "Disconnect"),
    ("名称", "Name"),
    ("类型", "Type"),
    ("地址", "Address"),
    ("端口", "Port"),
    ("数据库", "Database"),
    ("用户名", "Username"),
    ("密码", "Password"),
    ("备注", "Note"),
    ("启用", "Enabled"),
    ("策略", "Policy"),
    ("添加策略", "Add Policy"),
    ("更新策略", "Update Policy"),
    ("策略管理", "Policy Management"),
    ("认证", "Auth"),
    ("认证方法", "Auth Method"),
    ("网络", "Network"),
    ("环境", "Environment"),
    ("引用配置", "Reference Config"),
    ("选择类型…", "Select type…"),
    ("选择策略类型…", "Select policy type…"),
    ("数据(JSON)", "Data (JSON)"),
    ("保存连接后生效", "Applies after saving"),
    ("校验服务器证书", "Verify Server Certificate"),
    // 通用
    ("确定", "OK"),
    ("关闭", "Close"),
    ("新建", "New"),
    ("编辑", "Edit"),
    ("添加", "Add"),
    ("更新", "Update"),
    ("移除", "Remove"),
    ("全部", "All"),
    ("无", "None"),
    // 连接对话框 · 字段 / placeholder / 分区
    ("名称（如 生产 PG）", "Name (e.g. prod PG)"),
    ("备注（可选）", "Note (optional)"),
    ("搜索类型 / 驱动（回车选中）", "Search type / driver (Enter to select)"),
    ("prod, core（逗号分隔）", "prod, core (comma-separated)"),
    ("CA 证书路径（可选）", "CA cert path (optional)"),
    ("客户端证书路径（可选）", "Client cert path (optional)"),
    ("私钥路径（可选）", "Private key path (optional)"),
    ("可选，留空表示全部", "Optional; blank = all"),
    ("网络配置", "Network Config"),
    ("引用网络档案：", "Referenced network profile:"),
    ("（不引用：直连）", "(none: direct)"),
    ("管理", "Manage"),
    ("本机客户端", "Local client"),
    ("目标数据库", "Target database"),
    ("环境", "Environment"),
    ("选择环境…", "Select environment…"),
    ("管理环境", "Manage Environments"),
    ("已覆盖", "Overridden"),
    ("默认", "Default"),
    ("安全策略（覆盖环境默认）", "Security policy (overrides environment default)"),
    ("打开文件…", "Open File…"),
    ("新建文件…", "New File…"),
    ("模式", "Mode"),
    ("标签", "Tags"),
    ("连接设置", "Connection Settings"),
    ("数据库认证", "Database Auth"),
    ("连接安全（SSL/TLS）", "Connection Security (SSL/TLS)"),
    ("组织（标签 / 分组）", "Organization (Tags / Groups)"),
    ("暂存列表", "Staging List"),
    ("数据库类型", "Database Type"),
    ("项目", "Project"),
    ("名称", "Name"),
    ("备注", "Note"),
    ("驱动", "Driver"),
    ("收起", "Collapse"),
    ("详情", "Details"),
    ("复制", "Copy"),
    ("从全局定义同步", "Sync from Global"),
    ("测试连接（不改动已保存的配置）", "Test connection (does not modify saved config)"),
    ("关闭对话框（Esc）：草稿会保留", "Close dialog (Esc): drafts are kept"),
    ("保存（仅暂存草稿箱，不写入数据库；保存并退出时统一提交）", "Save (stages only to drafts; committed on Save & Exit)"),
    ("批量提交暂存栏全部条目到数据库并清空草稿箱，然后关闭", "Commit all staged items to the database, clear drafts, then close"),
    ("保存并退出", "Save & Exit"),
    ("编辑数据源连接", "Edit Data Source Connection"),
    ("新建数据源连接", "New Data Source Connection"),
    ("选择驱动实现…", "Select driver…"),
    ("该类型暂无可用驱动", "No drivers available for this type"),
    ("先选类型", "Select a type first"),
    ("仅全局不需要", "Not needed for global-only"),
    ("选择项目…", "Select project…"),
    ("选择认证方法…", "Select auth method…"),
    ("引用已保存配置…", "Reference saved config…"),
    ("选择 SSL 模式…", "Select SSL mode…"),
    ("未启用 TLS（disable / prefer / require / verify-ca / verify-full）", "TLS off (disable / prefer / require / verify-ca / verify-full)"),
    ("CA 证书", "CA Certificate"),
    ("客户端证书", "Client Certificate"),
    ("私钥", "Private Key"),
    ("多个标签用逗号分隔（导航检索用）", "Separate tags with commas (for navigation search)"),
    ("分组：打开项目后可用（分组为项目级）", "Group: available after opening a project (groups are project-scoped)"),
    ("分组：本项目暂无分组", "Group: no groups in this project"),
    ("分组", "Group"),
    // 连接对话框 · 作用域 / 状态
    ("仅全局", "Global Only"),
    ("仅项目", "Project Only"),
    ("全局+项目", "Global + Project"),
    ("该驱动未声明任何能力（drivers.capabilities 为空）", "This driver declares no capabilities (drivers.capabilities is empty)"),
    ("未命名连接", "Untitled Connection"),
    ("新建数据源", "New Data Source"),
    // 连接对话框 · 校验提示 / 帮助
    ("请先在左侧选择数据库类型", "Select a database type on the left first"),
    ("请选择驱动实现", "Select a driver implementation"),
    ("请填写连接名称", "Enter a connection name"),
    ("请填写连接 URI", "Enter a connection URI"),
    ("请选择或新建数据库文件（常规 → 连接设置）", "Select or create a database file (General → Connection Settings)"),
    ("展开分组", "Expand group"),
    ("收起分组", "Collapse group"),
    ("地址", "Address"),
    ("URI", "URI"),
    ("端口", "Port"),
    ("路径", "Path"),
    ("选择数据库类型与驱动后填写连接地址", "Choose a type and driver, then enter the connection address"),
    ("数据库文件", "Database File"),
    // 连接对话框 · 档案管理
    ("认证配置管理（AuthConfigManager）", "Auth Config Manager"),
    ("网络配置管理（NetworkConfigManager）", "Network Config Manager"),
    ("环境管理（EnvironmentManager）", "Environment Manager"),
    ("暂无配置，点击下方「新建」", "No configs; click New below"),
    ("策略", "Policy"),
    ("编辑", "Edit"),
    ("类型", "Type"),
    ("编辑配置", "Edit Config"),
    ("新建配置", "New Config"),
    ("取消编辑", "Cancel Edit"),
    ("该环境暂无策略", "No policies in this environment"),
    ("停用", "Disabled"),
    ("新建环境", "New Environment"),
    ("请选择策略类型", "Select a policy type"),
    ("名称不能为空", "Name cannot be empty"),
    // 连接对话框 · 草稿 / 类型树 / 项目
    ("删除这条草稿", "Delete this draft"),
    ("暂无草稿（点右上「添加」新建一条）", "No drafts (use Add in the top-right to create one)"),
    ("关系型", "Relational"),
    ("文件型", "File-based"),
    ("分析型", "Analytics"),
    ("暂无驱动", "No drivers"),
    ("类型目录为空（未读到 data_source_types）", "Type catalog is empty (no data_source_types loaded)"),
    ("没有匹配的类型 / 驱动", "No matching type / driver"),
    ("＋ 新增项目", "+ New Project"),
    ("打开现有目录…", "Open existing folder…"),
    ("不需要项目（仅全局）", "No project (global only)"),
];

/// 取当前语言下的文案：`tr("数据源")`。
///
/// 中文原文即 key；英文模式下查表，未登记项回退中文。UI 文案都是字面量，
/// 故签名要求 `&'static str`（避免泄漏非静态输入）。
pub fn tr(zh: &'static str) -> &'static str {
    if current() == Locale::En {
        for (k, v) in EN {
            if *k == zh {
                return v;
            }
        }
    }
    zh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_code_roundtrip() {
        assert_eq!(Locale::Zh.code(), "zh");
        assert_eq!(Locale::En.code(), "en");
        assert_eq!(Locale::from_code("zh"), Locale::Zh);
        assert_eq!(Locale::from_code("en"), Locale::En);
        assert_eq!(Locale::from_code("unknown"), Locale::Zh);
    }

    #[test]
    fn tr_zh_returns_original() {
        set_locale(Locale::Zh);
        assert_eq!(tr("数据源"), "数据源");
        assert_eq!(tr("未登记词"), "未登记词");
    }

    #[test]
    fn tr_en_resolves_registered() {
        set_locale(Locale::En);
        assert_eq!(tr("数据源"), "Data Sources");
        assert_eq!(tr("保存"), "Save");
        // 未登记回退中文
        assert_eq!(tr("未登记词"), "未登记词");
    }

    #[test]
    fn locale_switch_is_global() {
        set_locale(Locale::Zh);
        assert_eq!(current(), Locale::Zh);
        set_locale(Locale::En);
        assert_eq!(current(), Locale::En);
    }
}
