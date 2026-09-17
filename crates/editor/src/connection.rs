//! 连接绑定（B1）：文档绑定哪个连接、以及“有哪些连接可选”的端口
//!
//! ## 为什么要这一层
//!
//! 1a 的执行走的是引擎的 `DEFAULT_CONN_KEY = "active"`（**当前活动连接**）：用户在导航里
//! 点了哪个库，SQL 就发到哪个库——界面上**没有任何地方看得出这件事**（架构 §12 #26）。
//! B1 把“这条 SQL 发到哪”变成**文档属性**：绑定存在文档上（与 `mode` / `read_only` 同类），
//! 执行时随目标一起交给执行通道。
//!
//! ## 依赖方向
//!
//! 连接列表与建连都是**宿主的事**（workbench 侧才有 M3/M4 的加载器与连接管理器），
//! 因此这里只定义**端口**（[`ConnectionsPort`]）与展示所需的纯数据（[`ConnectionOption`]）：
//! 编辑器不依赖 `connection` / `database`，也不碰 I/O。
//!
//! ## 短码从哪来
//!
//! `P` / `G` / `GP`（项目 / 全局 / 共享快照）的口径**由宿主填**（`ConnectionOption.short`），
//! 避免在编辑器里再抄一份 id 前缀解析（那已经是 `engine::persistence::id_prefix` 与
//! `database::model::ConnectionScope` 的地盘）。

use std::rc::Rc;

/// 一个可绑定的连接（渲染路径只读的快照数据）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionOption {
    /// 稳定 id——绑定的就是它
    pub id: String,
    /// 作用域短码（`P` / `G` / `GP`；口径由宿主决定）
    pub short: String,
    /// 展示名（连接名）
    pub name: String,
    /// 运行态：是否已建连（宿主维护的真实状态，不猜）
    pub connected: bool,
    /// 【B10】驱动类型（`mysql_native` / `postgres_native` / `sqlite` / `duckdb` …）
    ///
    /// 编辑器用它决定**格式化 / 执行计划 / 转译用哪套方言**（同一个连接、两种引擎时，
    /// 方言不能猜）。宿主填（它才知道连接是什么驱动）。
    pub db_type: String,
}

/// 连接端口（宿主注入；未注入 = 没有可选项，且建连请求明确失败）
pub trait ConnectionsPort {
    /// 当前作用域下可选的连接（**必须是内存快照**：工具栏每帧都会读它）
    fn options(&self) -> Vec<ConnectionOption>;

    /// 确保某个连接已建连（未连则建连）；失败给**可读原因**（自动建连同 M4 口径）
    fn ensure_connected(&self, conn_id: &str) -> Result<(), String>;
}

/// 端口句柄（与 `QueryRunner` / `SessionStore` 同一模式：宿主注入、可缺省）
pub type ConnectionsHandle = Rc<dyn ConnectionsPort>;

/// 连接选择器的展示数据（纯函数算出来的，便于穷举断言）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionChip {
    /// 短码（`P` / `G` / `GP`）
    pub short: String,
    /// 展示名（连接名；认不出来的 id 就用 id 本身，不做美化）
    pub name: String,
    /// 运行态点：已建连为实心
    pub connected: bool,
}

impl ConnectionChip {
    /// 状态栏 / 工具栏文案：`●P·orders` / `○P·orders`
    ///
    /// 运行态用 `●` / `○` 而不是图标：这一处需要**文本可断言**（状态栏本来就是一行字），
    /// 图标留给工具栏按钮。
    pub fn text(&self) -> String {
        format!(
            "{}{}·{}",
            if self.connected { "●" } else { "○" },
            self.short,
            self.name
        )
    }
}

/// 未绑定连接时的状态栏文案（说明“会跟随当前连接”，而不是含糊的“无”）
pub const UNBOUND_LABEL: &str = "○ 未绑定连接";

/// 把绑定的 id 解析成展示数据；`None` = 解析不出（宿主没给这个连接 / 没接端口）
pub fn chip_for(
    conn_id: Option<&str>,
    options: &[ConnectionOption],
) -> Option<ConnectionChip> {
    let id = conn_id?;
    Some(match options.iter().find(|option| option.id == id) {
        Some(option) => ConnectionChip {
            short: option.short.clone(),
            name: option.name.clone(),
            connected: option.connected,
        },
        // 绑定还在、列表里没有（连接被删 / 换了项目）：显示 id，**不假装它还在**
        None => ConnectionChip {
            short: "?".to_string(),
            name: id.to_string(),
            connected: false,
        },
    })
}

/// 状态栏那一段文案（`None` 绑定 → [`UNBOUND_LABEL`]）
pub fn status_text(conn_id: Option<&str>, options: &[ConnectionOption]) -> String {
    match chip_for(conn_id, options) {
        Some(chip) => chip.text(),
        None => UNBOUND_LABEL.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ConnectionChip, ConnectionOption, UNBOUND_LABEL, chip_for, status_text};

    fn option(id: &str, name: &str, connected: bool) -> ConnectionOption {
        ConnectionOption {
            id: id.to_string(),
            short: if id.starts_with('G') { "G" } else { "P" }.to_string(),
            name: name.to_string(),
            connected,
            db_type: "mysql_native".to_string(),
        }
    }

    #[test]
    fn chip_text_marks_runtime_state() {
        let options = vec![
            option("P_orders", "orders", true),
            option("G_lab", "lab", false),
        ];
        assert_eq!(
            chip_for(Some("P_orders"), &options).expect("能解析").text(),
            "●P·orders"
        );
        assert_eq!(
            chip_for(Some("G_lab"), &options).expect("能解析").text(),
            "○G·lab"
        );
    }

    #[test]
    fn unbound_says_what_it_means() {
        // 未绑定不是“无连接”：执行仍会走当前活动连接（1a 口径），要如实说明
        let text = status_text(None, &[]);
        assert_eq!(text, UNBOUND_LABEL);
        assert!(text.contains("未绑定"));
        assert!(chip_for(None, &[]).is_none());
    }

    #[test]
    fn a_vanished_binding_shows_the_id_instead_of_a_pretend_name() {
        let text = status_text(Some("P_gone"), &[option("G_lab", "lab", true)]);
        assert!(text.contains("P_gone"), "认不出来就显示 id：{text}");
        assert!(text.contains('?'), "短码未知要看得出来：{text}");
        assert!(!text.contains('●'), "列表里没有就不能说它已连接：{text}");
    }

    #[test]
    fn chip_matches_the_bound_id_only() {
        let options = vec![option("P_a", "a", true), option("P_b", "b", true)];
        assert_eq!(chip_for(Some("P_b"), &options).expect("解析").name, "b");
        assert_ne!(chip_for(Some("P_b"), &options), chip_for(Some("P_a"), &options));
        // 大小写不同就是不同的 id（宿主给的 id 是稳定的，不做模糊匹配）
        assert!(chip_for(Some("p_b"), &options).is_some_and(|chip| chip.short == "?"));
    }

    #[test]
    fn chip_equality_is_by_value() {
        let chip = ConnectionChip {
            short: "P".to_string(),
            name: "orders".to_string(),
            connected: true,
        };
        assert_eq!(chip.clone().text(), chip.text());
    }
}
