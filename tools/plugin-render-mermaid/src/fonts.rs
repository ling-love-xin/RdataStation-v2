//! 嵌入式字体：把随 wasm 打包的 DejaVu Sans 子集注入上游 text_metrics 的 fontdb。
//!
//! wasm32-unknown-unknown 无系统字体，`load_system_fonts` 是空操作；没有字体时
//! 文本测量走 fallback 估算（SVG 仍可产出，但宽度与真实字体有偏差）。本模块在
//! 首次渲染前把子集字体注册进上游补丁的 `register_font`，使字体栈中的
//! `"DejaVu Sans"`（`mermaid_default` / `modern` 预设均含）真实命中。
//!
//! 子集来源：DejaVu Sans 2.37（Bitstream Vera 衍生，自由许可，允许嵌入/再分发），
//! 用 fonttools pyftsubset 裁剪为常用西文/标点/符号集（保留 family 名）。
//! 生成命令见 README.md「字体子集化」。

use std::sync::Once;

/// 子集字体（约 200KB）：拉丁 + 拉丁扩展 + 希腊 + 西里尔 + 通用标点/符号 + 箭头/数学/几何。
const DEJAVU_SANS_SUBSET: &[u8] = include_bytes!("../assets/DejaVuSans.subset.ttf");

static REGISTER: Once = Once::new();

/// 幂等注册（只执行一次；Lazy 单例的 TextMeasurer 在首次渲染时才初始化，
/// 因此首次渲染前注册即可保证注入生效）。
pub fn ensure_fonts() {
    REGISTER.call_once(|| {
        mermaid_rs_renderer::text_metrics::register_font(DEJAVU_SANS_SUBSET.to_vec());
    });
}
