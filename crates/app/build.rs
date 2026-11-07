//! 构建脚本：Windows 下把 `assets/icons/icon.ico` 嵌入 exe 资源。
//!
//! GPUI 的 Windows 平台注册窗口类时从当前模块资源 **ID=1** 加载图标
//! （见 gpui-pre-windows 的 `load_icon`：`LoadImageW(module, PCWSTR(1), IMAGE_ICON, ...)`），
//! 因此把应用图标以 ID=1 嵌入 exe，即可让窗口图标与任务栏图标生效。
//!
//! **清单（manifest）不要在这里再嵌一份。** 现状：
//! - `gpui-pre` 0.3.11 的 build.rs 在其默认 feature `windows-manifest` 下用
//!   `embed-resource` 编 `resources/windows/gpui.rc`，清单资源 ID=1；
//!   内容已覆盖 gpui-fast 的全部要求（`asInvoker`、Win10 supportedOS
//!   `{8e0f7a12-...}`、`dpiAware true/pm`、`dpiAwareness PerMonitorV2`、
//!   `heapType SegmentHeap`、comctl32 v6）。
//! - `gpui-kit` 带 `gpui-fast` 时给 `gpui-fast` 开的是
//!   `windows-manifest-provided-by-host`，gpui-fast 因此**不**自嵌清单。
//! - 于是本项目（gpui-pre + gpui-fast 双核心）的清单由 gpui-pre 提供；
//!   若本脚本再 `set_manifest`，链接会报
//!   `CVTRES: fatal error CVT1100: 资源重复，类型 MANIFEST，名称 1` → `LNK1123`。
//!
//! 只有将来**去掉 gpui-pre、只留 gpui-fast** 时才需要在本脚本里补一份清单
//! （届时 gpui-fast 自己也不嵌了，exe 会完全没有清单，高 DPI 会模糊）。

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let icon =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon.to_str().expect("icon.ico 路径需为合法 UTF-8"));
        // 缺少 rc.exe（Windows SDK）时不阻断构建，仅告警提示。
        if let Err(e) = res.compile() {
            println!("cargo:warning=嵌入应用图标失败（任务栏图标将回退系统默认）: {e}");
        }
    }
}
