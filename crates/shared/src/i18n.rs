//! 界面国际化：中英双语资源层（目录化资源文件）。
//!
//! 资源：
//! - `crates/shared/locales/zh.json`：源语言资源（键=中文原文，值=中文原文，翻译基线）；
//! - `crates/shared/locales/en.json`：英文翻译（键=中文原文，值=英文）。
//! 编译期 `include_str!` 嵌入（零运行时 IO、无部署资源缺失风险）；首次 `tr()` 查表时
//! 惰性解析一次（`once_cell::sync::Lazy`），键值借用嵌入的 `'static` 内存，返回 `&'static str`。
//!
//! 约定（design-guides §Internationalization）：
//! - **中文原文是 key**（项目界面语言，stable intent 的过渡形态；改动最小——字符串就地包 `tr(...)`）；
//! - 英文为翻译；`tr()` 按当前 [`Locale`] 查表，未登记词回退中文原文；
//! - 写每个语言，不翻译其形状：英文词序 / 句法独立定，不逐字对译；
//! - 复用的同一词（如“保存”）全仓同译，保持产品词汇表一致。
//!
//! 维护流程：新增界面文案 = 改 `tr("中文原文")` 调用 + **同时**在 `zh.json` / `en.json`
//! 各加一行；测试 `translation_zh_en_keys_are_aligned` 保证两文件键集合一致（漏译/误植即失败）。
//!
//! Locale 全局切换（`set_locale`）由设置页 `appearance.locale` 的 effect 驱动；
//! 不依赖 gpui（本 crate 是纯 Rust 基础层），跨线程安全。

use once_cell::sync::Lazy;
use std::collections::HashMap;

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

/// 编译期嵌入的语言资源（目录化：`crates/shared/locales/`）。
const ZH_JSON: &str = include_str!("../locales/zh.json");
const EN_JSON: &str = include_str!("../locales/en.json");

/// 英文翻译表（中文原文 → 英文）。惰性解析一次；键值借用 `EN_JSON` 的 `'static` 内存，
/// `tr()` 返回值维持 `&'static str`（与旧静态表签名一致，调用点零改动）。
static EN: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    serde_json::from_str(EN_JSON).expect("en.json 必须是合法 JSON 键值对象（键=中文原文，值=英文）")
});

/// 源语言资源（中文原文集合，完整性校验基线）。
static ZH: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    serde_json::from_str(ZH_JSON).expect("zh.json 必须是合法 JSON 键值对象（键=值=中文原文）")
});

/// 取当前语言下的文案：`tr("数据源")`。
///
/// 中文原文即 key；英文模式下查表，未登记项回退中文。UI 文案都是字面量，
/// 故签名要求 `&'static str`（避免泄漏非静态输入）。
pub fn tr(zh: &'static str) -> &'static str {
    if current() == Locale::En {
        if let Some(en) = EN.get(zh) {
            return en;
        }
    }
    zh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LOCALE` 是进程级全局：任何读写它的测试必须持锁，避免并行线程互相抢占
    /// （set 与断言之间被另一测试改写 → 偶发断言失败）。表（ZH/EN）只读无需锁。
    static LOCALE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn locale_code_roundtrip() {
        let _guard = LOCALE_TEST_LOCK.lock().unwrap();
        assert_eq!(Locale::Zh.code(), "zh");
        assert_eq!(Locale::En.code(), "en");
        assert_eq!(Locale::from_code("zh"), Locale::Zh);
        assert_eq!(Locale::from_code("en"), Locale::En);
        assert_eq!(Locale::from_code("unknown"), Locale::Zh);
    }

    #[test]
    fn tr_zh_returns_original() {
        let _guard = LOCALE_TEST_LOCK.lock().unwrap();
        set_locale(Locale::Zh);
        assert_eq!(tr("数据源"), "数据源");
        assert_eq!(tr("未登记词"), "未登记词");
    }

    #[test]
    fn tr_en_resolves_registered() {
        let _guard = LOCALE_TEST_LOCK.lock().unwrap();
        set_locale(Locale::En);
        assert_eq!(tr("数据源"), "Data Sources");
        assert_eq!(tr("保存"), "Save");
        // 未登记回退中文
        assert_eq!(tr("未登记词"), "未登记词");
    }

    #[test]
    fn locale_switch_is_global() {
        let _guard = LOCALE_TEST_LOCK.lock().unwrap();
        set_locale(Locale::Zh);
        assert_eq!(current(), Locale::Zh);
        set_locale(Locale::En);
        assert_eq!(current(), Locale::En);
    }

    /// 目录化完整性：zh / en 键集合完全一致（漏译、误植键、多余键都会让本测试失败）。
    /// 资源文件本身合法（`Lazy` 已成功解析）由访问 `&*ZH` / `&*EN` 触发断言。
    #[test]
    fn translation_zh_en_keys_are_aligned() {
        let zh = &*ZH;
        let en = &*EN;
        let missing_in_en: Vec<_> = zh.keys().filter(|k| !en.contains_key(*k)).collect();
        let extra_in_en: Vec<_> = en.keys().filter(|k| !zh.contains_key(*k)).collect();
        assert!(missing_in_en.is_empty(), "en 缺失翻译键: {:?}", missing_in_en);
        assert!(extra_in_en.is_empty(), "en 含 zh 没有的键: {:?}", extra_in_en);
    }

    /// 目录化资源在 En 模式下被 `tr()` 实际使用（防静态表时代遗留）。
    #[test]
    fn tr_uses_directory_resources() {
        let _guard = LOCALE_TEST_LOCK.lock().unwrap();
        set_locale(Locale::En);
        // 抽样验证：资源层来自 locales/en.json（非内嵌表）。
        assert_eq!(tr("连接"), "Connection");
        assert_eq!(tr("测试连接"), "Test Connection");
        set_locale(Locale::Zh);
    }
}
