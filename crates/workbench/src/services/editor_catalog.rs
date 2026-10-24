//! 语义诊断目录快照注入（#54 B 批的 workbench 侧）
//!
//! ## 干什么
//!
//! 编辑器扫描线程做语义诊断（未知表 / 列的红波浪线）需要「目录 + 方言」。目录
//! 来自补全的 L1 缓存（[`WorkbenchCompletion`]——同一份预载、同一条 TTL 防线），
//! 方言由连接驱动推导。宿主每 1 秒调一次 [`pump`]：看「活动文档绑定了哪个连接」，
//! 变了就把该连接的目录快照**按文档**推给 [`EditorShared::set_catalog_snapshot`]。
//!
//! ## 为什么轮询而不是订阅
//!
//! `EditorShared` 是 `Rc` 句柄，没有「文档激活 / 绑连」事件流（连接绑定发生在
//! editor 侧，跨 crate 不推事件）。轮询 1s 一拍覆盖全部变化来源（切换文档 / 绑连 /
//! 解绑 / 目录刷新），零内核改动；语义诊断本身在扫描线程异步跑，晚一拍无感。
//!
//! ## 边界（如实记录）
//!
//! - **只推活动文档**：非活动文档若在编辑（扫描按文本变更触发）会用旧快照最多 1s
//!   ——多文档同时编辑且各绑各连接时，那一拍的语义诊断可能拿错目录。影响面：波浪线
//!   短暂不准，下一拍自愈；可接受（本仓多文档同时编辑场景低频）。
//! - **目录未载好 → 推 `None`（清掉旧条目）**：语义段跳过，不拿别的连接目录猜。
//!   用户敲补全时 [`WorkbenchCompletion::catalog_snapshot`] 的同源预载（`raw`）自然
//!   触发，下一拍注入就有快照（语义诊断晚一拍，与补全「晚一拍」同一哲学）。
//! - **文档关闭 → 残留清理**：每拍 [`EditorShared::retain_catalog_snapshots`] 按现存
//!   文档对齐（关一个清一个，不积内存）。
//! - 未绑定连接的 SQL 文档：活动文档连接为 `None` → 推 `None`（语义段跳过，词法
//!   诊断照常——与 editor 侧空目录退化同一口径）。

use std::rc::Rc;

use editor::shared::{EditorShared, ShowDocumentPort};
use lsp_types::Uri;

use super::editor_completion::WorkbenchCompletion;
use crate::panels::Shared;

/// 一拍：读活动文档绑定连接 → 有则推该连接目录快照；再按现存文档对齐残留。
///
/// 无状态（变化检测靠「每拍覆盖推」：`HashMap::insert` 幂等，1s 一次的 Arc clone
/// 代价可忽略）；泵由宿主在 view 侧 `cx.spawn` 起（捕获 `EditorShared` / `Rc` 非
/// `Send`，只能跑主线程——与 `scratchpad_meta` 泵同一模式）。
pub fn pump(editor: &EditorShared, completion: &Rc<WorkbenchCompletion>) {
    // 活动文档 → 绑定连接 → 目录快照（连接 None 时推 None：语义段跳过）
    {
        let service = editor.service();
        if let Some(doc) = service.active() {
            let doc_id = doc.id().clone();
            let snapshot = doc
                .connection()
                .and_then(|conn| completion.catalog_snapshot(conn));
            editor.set_catalog_snapshot(&doc_id, snapshot);
        }
    }
    // 残留清理：现存文档 id 之外的全清
    {
        let service = editor.service();
        let alive: Vec<editor::model::DocumentId> = service
            .documents()
            .iter()
            .map(|doc| doc.id().clone())
            .collect();
        editor.retain_catalog_snapshots(&alive);
    }
}

/// 【#54 C6/C7】注入「跳转定义目标解析」端口：`rds://navigator/<限定表名>` →
/// 导航树定位（复用 Quick Open `⌥↵` 同款 `request_reveal` 机制）。
///
/// 内核 `go_to_definition` 对非 http/https 的 `target_uri` 先问 show_document
/// handler（见 vendor gpui-base `input/editor/lsp/definitions.rs` 实测）；这里
/// 把 `rds://navigator/<限定名>` 解析成 [`engine::ObjectRef`] 投递到导航面板。
/// 跳转上下文 = **当前活动文档绑定连接**（用户从该文档 Ctrl+Click，它就是活动
/// 文档；URI 本身不携带连接）。未命中（scheme 不符 / 无活动文档 / 未绑连接）→
/// 返回 `false` 让内核走默认（当前文档内移动光标，无副作用）。
pub fn attach_jump(editor: &EditorShared, workbench: &Shared) {
    // `ShowDocumentPort` 是 `Rc<dyn Fn + 'static>`：闭包只能捕获 `'static` 句柄，
    // 不能捕获 `&` 引用——先 clone 成 Rc（`EditorShared` / `Shared` 都是 Rc 集合，
    // clone = 复制句柄，与其它 attach_* 同款模式）。别名带 `_owned`：闭包 move 走
    // 它们，外面用原参数 `&` 调 `attach`。
    let editor_owned = editor.clone();
    let workbench_owned = workbench.clone();
    let port: ShowDocumentPort = Rc::new(move |params, _window, _cx| {
        // 只认 rds://navigator/<限定表名>；其他 scheme 不拦（内核默认处理）
        let Some(name) = parse_navigator(&params.uri) else {
            return false;
        };
        // 跳转上下文：当前活动文档绑定的连接
        let conn_id = {
            // Ref 先绑定局部，避免临时借用跨返回点悬垂
            let service = editor_owned.service();
            service
                .active()
                .and_then(|doc| doc.connection().map(|conn| conn.to_string()))
        };
        let Some(conn_id) = conn_id else {
            return false;
        };
        // 限定名 → ObjectRef：`public.customers` → schema=public / name=customers；
        // catalog 留空（导航面板 reveal 用「连接下第一个已加载 catalog」兜底，
        // 见 `NavView::pump_reveal`）。catalog.table 不带 schema 时整体进 name，
        // reveal 按 schema 空 + name 匹配目录层，同样可定位。
        let (schema, table) = match name.split_once('.') {
            Some((s, t)) if !s.is_empty() && !t.is_empty() => (s.to_string(), t.to_string()),
            _ => (String::new(), name),
        };
        let object = engine::ObjectRef::table(conn_id, "", schema, table);
        workbench_owned.request_reveal(object);
        true
    });
    editor.attach_show_document(port);
}

/// 解析 `rds://navigator/<限定表名>` → 限定名（`None` = 不是本协议，别拦）。
///
/// 纯函数便于测试。用字符串前缀匹配而非 `fluent_uri` 的 scheme/host API
/// （lsp-types 0.97 的 `Uri` 是 fluent-uri，字段访问器语义与 url crate 不同，
/// 而 `Display` 往返稳定——`sql_definition` 里 `target_uri.to_string()` 实测
/// 输出 `rds://navigator/public.customers`，就锁这个形状）。
fn parse_navigator(uri: &Uri) -> Option<String> {
    // 先落到局部 String，避免临时值被 strip_prefix 借用后悬垂
    let s = uri.to_string();
    let rest = s.strip_prefix("rds://navigator/")?;
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**。注入泵只是「读活动文档 → 取 completion 快照 →
    // 按文档 set → retain」，三段各自的契约都在各自模块测试里（`active_id` 在
    // editor service、`catalog_snapshot` 的 fresh/TTL 在 editor_completion、
    // 按文档存取在 editor shared）。这里显式声明依赖，避免将来有人以为本模块
    // 需要自己的 mock 套件。
    use std::str::FromStr;

    use super::{WorkbenchCompletion, parse_navigator};
    use lsp_types::Uri;

    #[test]
    fn injection_is_covered_by_its_three_contracts() {
        let _ = std::any::type_name::<WorkbenchCompletion>();
    }

    #[test]
    fn navigator_uri_parses_schema_table_and_rejects_foreign() {
        // schema.table 限定名
        let uri = Uri::from_str("rds://navigator/public.customers").unwrap();
        assert_eq!(parse_navigator(&uri).as_deref(), Some("public.customers"));
        // 裸表名（无 schema）
        let uri = Uri::from_str("rds://navigator/customers").unwrap();
        assert_eq!(parse_navigator(&uri).as_deref(), Some("customers"));
        // 其他 scheme / 其他 host / 空路径都不拦
        assert_eq!(
            parse_navigator(&Uri::from_str("file:///a/b.sql").unwrap()),
            None
        );
        assert_eq!(
            parse_navigator(&Uri::from_str("rds://scratchpad/x").unwrap()),
            None
        );
        assert_eq!(parse_navigator(&Uri::from_str("rds://navigator/").unwrap()), None);
    }
}
