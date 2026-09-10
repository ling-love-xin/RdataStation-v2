//! 构建脚本：Windows 下把 `assets/icons/icon.ico` 嵌入 exe 资源。
//!
//! GPUI 的 Windows 平台注册窗口类时从当前模块资源 **ID=1** 加载图标
//! （见 gpui-pre-windows 的 `load_icon`：`LoadImageW(module, PCWSTR(1), IMAGE_ICON, ...)`），
//! 因此把应用图标以 ID=1 嵌入 exe，即可让窗口图标与任务栏图标生效。

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/icon.ico");

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
