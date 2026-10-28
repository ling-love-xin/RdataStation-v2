//! 支撑能力：文本原语 / 持久化 / 折叠 / 历史 / 书签 / 端口与命令
//!
//! 被 core / exec / language / notebook / view 共用；多数为纯函数 + 宿主注入端口。

/// 【书签】编辑器接入 `rds-file-meta` 的纯逻辑（切换 / 跳转 / 排序），I/O 在 `view/host`
pub mod bookmark;
/// 动作声明（A10：键位 / 菜单动作的纯描述）
pub mod commands;
/// 【B1 切片一】文档绑定连接 / schema 面包屑 / 状态栏连接段
pub mod connection;
/// 【B6】执行错误定位（解析驱动错误文本中的行列）
pub mod diagnostics;
/// 文本操作纯函数（行注释开关等）
pub mod edit;
/// 【2c】文本编码与换行（检测 / 解码 / 编码 / 换行规范化的纯函数）
pub mod encoding;
/// 【B17】折叠候选纯函数（括号组 / `CASE`·`BEGIN`·`IF`·`LOOP` → `END` / 多行注释与字面量）
pub mod fold;
/// 【B10】格式化（有选区只动选区 / 光标按语句内偏移映射回去）
pub mod format;
/// 【2c】隐藏字符类别位掩码（零宽 / 特殊空格 / 双向控制 / 控制）
pub mod hidden;
/// 【B8】历史面板数据侧（相对时间文案 / 记录形状）
pub mod history;
/// 【2026-10-27】缩进操作纯函数（检测 / 转换 / 裁剪尾随空格，VSCode 缩进面板落地能力）
pub mod indent;
/// 【A9】打开 / 保存 / 外部修改检测
pub mod persist;
/// 【B12】项目只读端口（项目锁：写源库对象在提交前被拒）
pub mod project;
/// 【B18】后台扫描器（语句数 / 折叠候选 / 打字期诊断）
pub mod scan;
/// 【T1.6】联邦源清单（「源清单 ▾」浮层的数据侧与纯函数模型）
pub mod sources;
/// 【T2c-5】工具栏分组自定义布局（权威状态）
pub mod toolbar;
/// 【B10】方言转译（有选区只动选区 / 源方言只从连接来）
pub mod translate;
/// 本 crate 的结构尺寸常量（零裸值约定）
pub mod ui;
