# rds-render-mermaid-wasm

RdataStation 的 Mermaid 渲染插件（wasm / Extism PDK 插件）。

把 mermaid-rs-renderer 0.3.1 编译为 wasm32-unknown-unknown，宿主经 Extism 1.30
加载调用：**源文本或 JSON 信封进、SVG 出**，纯计算、无 host function 依赖（除诊断
通道 `host_log`）。

## 契约

导出函数 `render`：

- **纯文本**：mermaid 源文本（主题走源内 `%%{init}%%` 指令，mermaid.js 语义）
- **JSON 信封**：

  ```json
  {
    "source": "flowchart LR\n  A --> B\n",
    "theme": {
      "preset": "modern | mermaid_default | dark | forest | neutral",
      "overrides": { "primary_color": "#123456" }
    }
  }
  ```

  - `preset` 缺省 `modern`；未知预设回退 modern。
  - `overrides` 的键 = `mermaid_rs_renderer::theme::Theme` 的 pub 字段名
    （如 `font_family` / `primary_color`），覆盖 preset；未知键忽略。
  - **非 JSON / 缺 `source` → 按纯文本退化**（与宿主 render 第一刀兼容）。

输出为 UTF-8 SVG 文本。语法错误返回调用层错误（不崩溃）。

## 构建

```powershell
# 1. wasm32 target（本机已手动安装；rustup 镜像 404 时的替代方案见下）
rustup target add wasm32-unknown-unknown

# 2. 编译（独立 workspace，不入仓库根）
Set-Location tools\plugin-render-mermaid
cargo build --release --target wasm32-unknown-unknown
# 产物: target\wasm32-unknown-unknown\release\rds_render_mermaid_wasm.wasm (~2.6MB)
```

> **rustup 镜像 404 时的手动安装**（本仓 2026-10-16 实测路径）：
> 从 rsproxy.cn 下载 `rust-std-<版本>-wasm32-unknown-unknown.tar.xz`
> （如 `https://rsproxy.cn/dist/2026-08-20/rust-std-1.98.0-wasm32-unknown-unknown.tar.xz`），
> 解压后把 `rust-std-.../rust-std-wasm32-unknown-unknown/lib/rustlib/wasm32-unknown-unknown`
> （注意双层目录）复制到 `~/.rustup/toolchains/<toolchain>/lib/rustlib/`。

## 字体子集化（资产更新时）

嵌入 `assets/DejaVuSans.subset.ttf`（200KB，DejaVu Sans 2.37 子集，自由许可可再分发）。
重新生成：

```powershell
python -m fontTools.subset DejaVuSans.ttf --output-file=assets/DejaVuSans.subset.ttf `
  --unicodes=U+0000-00FF,U+0100-017F,U+0370-03FF,U+0400-04FF,U+2000-206F,U+2190-21FF,U+2200-22FF,U+25A0-25FF,U+4E00-9FFF
```

上游默认字体栈已含 `"DejaVu Sans"`（`mermaid_default` / `modern` 预设），插件首次
渲染前经 vendor 补丁 `text_metrics::register_font` 注入，`fontdb` query 自动命中。

## vendor 补丁（mermaid-rs-renderer 0.3.1，共 3 处）

1. `lib.rs`：`mod text_metrics` → `pub mod text_metrics`（暴露注入点）
2. `text_metrics.rs`：`INJECTED_FONTS` 全局 + `register_font(Vec<u8>)` +
   `TextMeasurer::new()` 灌入注入字体（wasm32 无系统字体，唯一字体来源）
3. **wasm 计时**：新增 `wasm_time.rs`（`Instant` 恒 0 stub）+ `edge_pipeline.rs` /
   `layout/mod.rs` 的 `use std::time::Instant` 在 `cfg(target_arch="wasm32")` 下换引。
   —— wasm32-unknown-unknown 的 `Instant::now()` 直接 panic（"time not implemented"），
   上游 stage 计时纯诊断用途，wasm 下恒 0 不影响 SVG 产物。

## 诊断

插件安装 `std::panic::set_hook`，panic message 经 `extism:host/user/host_log` 上报
（宿主注册同名 no-op Function，**必须注册否则实例化失败**）。生产可保留；宿主日志
侧未来可接真实落盘。

## 测试

```powershell
cargo test            # native：envelope 7 条 + 渲染 2 条（rlib 仅为此启用）
```
