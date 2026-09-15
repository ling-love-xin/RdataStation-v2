//! 洞察面板的**命令与 Action**（M8 Phase 1 落地）。
//!
//! 规划内容（见 `docs/architecture/insight/insight-prototype-design.md` §2.1 / §8）：
//! - `OpenInsight`：打开右 Dock 洞察面板（Quick Open 已有「打开洞察」项，见 `workbench::view`）
//! - `InsightRefresh`：重算当前目标画像（保持 Tab 与折叠态）
//! - `ReloadInsightRules`：规则热加载的兜底 / 排障入口（常规改动由目录监听处理）
//!
//! 本文件当前为空占位：视图与服务未就绪前不注册任何 Action，避免出现「有快捷键但无行为」。
