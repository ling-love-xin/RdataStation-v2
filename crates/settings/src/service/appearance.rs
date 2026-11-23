//! 外观设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use std::sync::RwLock;

use gpui_kit::App;
use gpui_kit::component::Theme;
use gpui_kit::px;

use super::{explicit_theme_mode, theme_mode_from_text};
use crate::SettingsService;
use crate::model::{AppearanceMode, Settings};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::window_material::WindowMaterial;

/// 主题方案（包）变更的副作用出口。
///
/// 为什么用 sink 而不是在设置层直接装包：包资产（`assets/themes/packs/<id>/`）与
/// `Theme` 装配属于**装配层**知识（主题目录从哪来、产品 token 怎么一起换），
/// 设置在 `crates/settings` 里只该管“值”。由 `crates/app/src/main.rs` 注册一次
/// （`apply_theme_pack_state`），未注册时（测试 / 无主题目录的宿主）只落盘不报错。
static THEME_PACK_SINK: RwLock<Option<fn(&mut App)>> = RwLock::new(None);

/// 注册主题方案变更出口（装配层调用一次；重复注册以最后一次为准）。
pub fn install_theme_pack_sink(sink: fn(&mut App)) {
    if let Ok(mut guard) = THEME_PACK_SINK.write() {
        *guard = Some(sink);
    }
}

fn notify_theme_pack(cx: &mut App) {
    if let Some(sink) = THEME_PACK_SINK.read().ok().and_then(|guard| *guard) {
        sink(cx);
    }
}

/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
        Slot::AppearanceThemeMode => {
            let Some(mode) = value.as_text().and_then(theme_mode_from_text) else {
                return false;
            };
            SettingsService::set_theme_mode(mode, window, cx);
        }
        Slot::AppearanceFontSize => {
            let Some(size) = value.as_number() else {
                return false;
            };
            // 只认 12–20px（页面只给预设档；设置文件被手改坏时拒绝，不猜）。
            if !(12.0..=20.0).contains(&size) {
                return false;
            }
            SettingsService::set_font_size(size as f32, window, cx);
        }
        Slot::AppearanceLocale => {
            let Some(code) = value.as_text() else {
                return false;
            };
            // 只认 zh / en（页面只给预设档；设置文件被手改坏时拒绝，不猜）。
            if !(code == "zh" || code == "en") {
                return false;
            }
            SettingsService::set_locale(&code, window, cx);
        }
        Slot::AppearanceTabBarMaxWidth => {
            let Some(w) = value.as_number() else {
                return false;
            };
            // 只认 100–600px（页面只给预设档；设置文件被手改坏时拒绝，不猜）。
            if !(100.0..=600.0).contains(&w) {
                return false;
            }
            {
                let settings = cx.global_mut::<Settings>();
                settings.appearance.tab_bar_max_width = w as f32;
            }
            persist(&cx.global::<Settings>().clone());
            // 落盘后由 workbench 的 `on_settings_changed` 装配到 DockSkin。
        }
        Slot::AppearanceTabBarVariant => {
            let Some(v) = value.as_text() else {
                return false;
            };
            // 只认 TabVariant 五态的落盘值（页面只给预设档；手改坏时拒绝，不猜）。
            if !matches!(v, "tab" | "outline" | "pill" | "segmented" | "underline") {
                return false;
            }
            {
                let settings = cx.global_mut::<Settings>();
                settings.appearance.tab_bar_variant = v.to_string();
            }
            persist(&cx.global::<Settings>().clone());
            // 落盘后由 workbench 的 `on_settings_changed` 装配到 DockSkin。
        }
        Slot::AppearanceThemePack => {
            let Some(id) = value.as_text() else {
                return false;
            };
            // 只认内置包（页面只给这几档；设置文件被手改坏时拒绝，不猜）。
            if !crate::theme_pack::is_builtin(id) {
                return false;
            }
            SettingsService::set_theme_pack(id, cx);
        }
        Slot::AppearanceWindowMaterial => {
            let Some(code) = value.as_text() else {
                return false;
            };
            // 只认值域里的三档（页面只给这几档；设置文件被手改坏时拒绝，不猜）。
            let Some(material) = WindowMaterial::from_code(code) else {
                return false;
            };
            SettingsService::set_window_material(material, cx);
        }
        _ => return false,
    }
    true
}

impl SettingsService {
    pub fn theme_mode(cx: &App) -> AppearanceMode {
        cx.global::<Settings>().appearance.theme_mode
    }
    /// 设置主题模式：更新 global → 持久化 → 即时生效。
    ///
    /// `System` 走 `Theme::sync_system_appearance`（按系统外观解析）；显式模式走
    /// `Theme::change`。
    pub fn set_theme_mode(
        mode: AppearanceMode,
        mut window: Option<&mut gpui_kit::Window>,
        cx: &mut App,
    ) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.appearance.theme_mode = mode;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        match mode {
            AppearanceMode::System => Theme::sync_system_appearance(window.as_deref_mut(), cx),
            AppearanceMode::Light | AppearanceMode::Dark => {
                Theme::change(
                    explicit_theme_mode(mode).expect("显式模式必有映射"),
                    window.as_deref_mut(),
                    cx,
                );
            }
        }
        // 主题配置（RDS 资产声明 `font.size: 16`）会经 `apply_config` 重载 `theme.font_size`，
        // 而窗口 rem 基准是用户字号——不重放一次，切换后 font_size 系与 rem 系错位，
        // 表现就是「切明暗字体大小变了」。与启动装配同序：先主题、后字号。
        Self::set_font_size(Self::font_size(cx), window, cx);
        // 材质的外壳 alpha 必须跟着主题颜色一起重放：`Theme::change` /
        // `sync_system_appearance` 会从资产重建 `ThemeColor`，重建出来的 alpha 恒为 1.0，
        // 不重放就等于「材质只在刚打开那一瞬生效」。
        Self::apply_window_material_alpha(cx);
    }
    /// 明暗切换（`ToggleThemeMode` 命令与设置页「主题模式」两条路径共用）。
    ///
    /// 显式模式下在两态间切换；`System` 下按当前系统外观反切到显式对侧
    /// （系统当前为明 → 落到深，反之亦然），一次按键即脱离跟随。
    pub fn toggle_theme_mode(window: Option<&mut gpui_kit::Window>, cx: &mut App) {
        let next = match Self::theme_mode(cx) {
            AppearanceMode::Light => AppearanceMode::Dark,
            AppearanceMode::Dark => AppearanceMode::Light,
            AppearanceMode::System => {
                let current = window
                    .as_ref()
                    .map(|window| window.appearance())
                    .unwrap_or_else(|| cx.window_appearance());
                match current {
                    gpui_kit::WindowAppearance::Dark => AppearanceMode::Light,
                    _ => AppearanceMode::Dark,
                }
            }
        };
        Self::set_theme_mode(next, window, cx);
    }
    /// 读取当前界面字号（px，默认 16）。
    pub fn font_size(cx: &App) -> f32 {
        cx.global::<Settings>().appearance.font_size
    }
    /// 设置并持久化界面字号，即时生效：**两个基准同步应用**——
    /// 主题字号（`Theme::update`，影响 `font_size * N` 类宽度）与窗口 rem 基准
    /// （`Window::set_rem_size`，rems / Tailwind 档位 / 布局全缩放）。
    /// 缺其一则出现「文字缩放、宽度不缩放」的错位（rem 系与 font_size 系是
    /// 两套独立的基准，窗口默认 16px，见 gpui `Window::rem_size`）。
    ///
    /// `window` 为 `None`（启动 / 测试）时遍历现有窗口逐个设；新窗口在
    /// `open_window` 回调里由装配层同步（见 `crates/app/src/main.rs`）。
    pub fn set_font_size(font_size: f32, window: Option<&mut gpui_kit::Window>, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.appearance.font_size = font_size;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        // 主题字号：`font_size * N` 类宽度（对话框宽度等）跟随缩放。
        Theme::update(cx, |theme| theme.font_size = px(font_size));
        match window {
            Some(window) => window.set_rem_size(px(font_size)),
            None => {
                for handle in cx.windows() {
                    // update 返回 Result（窗口可能已失效），此处无更优处理路径，显式忽略。
                    let _ =
                        handle.update(cx, |_view, window, _| window.set_rem_size(px(font_size)));
                }
            }
        }
        cx.refresh_windows();
    }
    /// 读取当前界面语言（默认中文）。
    pub fn locale(cx: &App) -> shared::i18n::Locale {
        shared::i18n::Locale::from_code(&cx.global::<Settings>().appearance.locale)
    }
    /// 读取当前主题包 id（默认 `"rds"` = 现役主题）。
    ///
    /// 注意：这是**落盘值**；试切换用环境变量 `RDS_THEME_PACK` 时实际生效的是环境变量，
    /// 优先级判定在装配层（`crates/app/src/theme_pack.rs::selected`）。
    pub fn theme_pack(cx: &App) -> String {
        cx.global::<Settings>().appearance.theme_pack.clone()
    }
    /// 设置并持久化主题包，再经 [`install_theme_pack_sink`] 让装配层即时重装。
    ///
    /// 换包必须「主题 + 产品 token」一起换：产品 token 只按明暗模式解析、没有主题维度，
    /// 只换主题会让活动栏底色 / Quick Open 组头 / 搜索命中底 / 结果集四槽残留上一包的值。
    pub fn set_theme_pack(id: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.appearance.theme_pack = id.to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_theme_pack(cx);
    }
    /// 读取窗口材质（默认「关闭」）。
    ///
    /// 手改坏的值按「关闭」处理而不报错：材质是显示项，坏值不该拦住启动。
    pub fn window_material(cx: &App) -> WindowMaterial {
        WindowMaterial::from_code(&cx.global::<Settings>().appearance.window_material)
            .unwrap_or(WindowMaterial::Off)
    }
    /// 设置并持久化窗口材质，即时生效：落盘 → 外壳 alpha 重放 → 重设**全部**窗口的
    /// 背景外观 → 刷新。
    ///
    /// 材质作用在**窗口**上（`set_background_appearance`），不在主题里；主题这边只做
    /// 外壳面 alpha 的覆盖（见 `apply_window_material_alpha`）。
    ///
    /// 为什么不分"只设当前窗口"：材质是全局外观项，主窗 / 设置窗 / 解锁窗同时存在，
    /// 只改改设置的那个窗会留下"一窗透一窗不透"的错位。
    pub fn set_window_material(material: WindowMaterial, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.appearance.window_material = material.code().to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        Self::apply_window_material_alpha(cx);
        Self::apply_window_material_to_all(cx);
        cx.refresh_windows();
    }
    /// 窗口创建回调里调用：按当前设置给**这一个**窗口设背景外观。
    ///
    /// 必须在 `open_window` 的回调里做：窗口建好之后再设会先出一帧不透明底色。
    pub fn apply_window_material(window: &mut gpui_kit::Window, cx: &App) {
        window.set_background_appearance(Self::window_material(cx).background_appearance());
    }
    /// 把当前材质的背景外观设到**所有**现有窗口（设置页切换时没有"目标窗口"）。
    pub fn apply_window_material_to_all(cx: &mut App) {
        let appearance = Self::window_material(cx).background_appearance();
        for handle in cx.windows() {
            // update 返回 Result（窗口可能已失效），此处无更优处理路径，显式忽略。
            let _ = handle.update(cx, |_view, window, _| {
                window.set_background_appearance(appearance)
            });
        }
    }
    /// 按当前材质重放**外壳面**的 alpha（关闭档写 1.0，避免上一次的透明残留）。
    ///
    /// 落点只有四个 token：`title_bar` / `status_bar` / `tab_bar` / `sidebar`。
    /// 内容面（`background` / `table` / `popover` / `list` / `input` …）**刻意不动**：
    /// 文字直接贴在上面，透明了对比度就靠系统壁纸的运气。
    pub fn apply_window_material_alpha(cx: &mut App) {
        let alpha = Self::window_material(cx).shell_alpha().unwrap_or(1.0);
        Theme::update(cx, |theme| {
            let colors = &mut theme.colors;
            colors.title_bar.a = alpha;
            colors.status_bar.a = alpha;
            colors.tab_bar.a = alpha;
            colors.sidebar.a = alpha;
        });
    }
    /// 设置并持久化界面语言，即时生效：更新 global → 写 settings.json →
    /// 同步 `shared::i18n` 全局语言（`tr()` 查表基准）→ 同步组件库内置文案 →
    /// 刷新所有窗口（全 UI 文案即时切换）。
    pub fn set_locale(code: &str, _window: Option<&mut gpui_kit::Window>, cx: &mut App) {
        let locale = shared::i18n::Locale::from_code(code);
        {
            let settings = cx.global_mut::<Settings>();
            settings.appearance.locale = locale.code().to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        shared::i18n::set_locale(locale);
        // 组件库自带文案（Dialog 的确认/取消、Select 占位、Pagination、Empty、Diff…）走
        // rust-i18n 的全局 locale，和我们自己的 `shared::i18n` 是两套：不同步的话组件文案
        // 会一直停在 fallback `en`（中文界面里蹦出 Cancel / No data）。映射见
        // `Locale::component_code`（存储码 `zh` ≠ 组件库语言键 `zh-CN`）。
        gpui_kit::component::set_locale(locale.component_code());
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod locale_link_tests {
    use shared::i18n::Locale;

    /// 组件库（rust-i18n）接受映射后的语言码，且读回来一致。
    ///
    /// 只钉「接线通路」：组件库的 `set_locale` 是全局 atomic，喂错码（如 `zh`）不会报错、
    /// 只会静默回退 `en`，所以这里显式断言喂进去的码就是读回来的码。
    #[test]
    fn component_locale_accepts_mapped_code() {
        for locale in [Locale::Zh, Locale::En] {
            gpui_kit::component::set_locale(locale.component_code());
            assert_eq!(&*gpui_kit::component::locale(), locale.component_code());
        }
        // 复位到中文：本 crate 只有这一处碰组件库 locale，复位是为了不给后续用例留状态。
        gpui_kit::component::set_locale(Locale::Zh.component_code());
    }
}
