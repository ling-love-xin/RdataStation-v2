//! 数据源映射：正文高亮切分、索引命中 / 草稿文件 / 连接清单 → 行。

use super::*;

/// 把 FTS5 `snippet()` 的 `<mark>…</mark>` 标记切成「普通 / 命中」段（纯函数，可单测）。
///
/// 不闭会的标记退化为普通文本（引擎不该产出这种，但别让 UI panic）。
pub fn markup_segments(markup: &str) -> Vec<(String, bool)> {
    const OPEN: &str = "<mark>";
    const CLOSE: &str = "</mark>";
    let mut out = Vec::new();
    let mut rest = markup;
    loop {
        let Some(start) = rest.find(OPEN) else {
            if !rest.is_empty() {
                out.push((rest.to_string(), false));
            }
            return out;
        };
        if start > 0 {
            out.push((rest[..start].to_string(), false));
        }
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            out.push((after.to_string(), false));
            return out;
        };
        out.push((after[..end].to_string(), true));
        rest = &after[end + CLOSE.len()..];
    }
}

/// 元数据行的业务键：`meta:` + 统一引用的 key。
///
/// 用 `ObjectRef::key()`（连接 / catalog / schema / 父对象 / 名字）而不是自己拼：
/// ① 与导航树节点、搜索命中同构——跨屏说的是同一个对象；
/// ② 历史上这里只拼「连接 + 种类 + 父对象 + 名字」，**不含 catalog / schema**，
///    于是 `sales.orders` 与 `archive.orders` 会撞成同一个键（选中跟错行）。
pub(crate) fn meta_key(object: &engine::ObjectRef) -> String {
    format!("meta:{}", object.key())
}

/// 索引命中 → 元数据对象（`None` = 该类别不立行）。
///
/// 引用的构造（类别串解析、NULL 段收敛）与导航搜索结果走**同一个**入口
/// （`ObjectRef::from_index_hit`）；类别 → 属性面板类别的映射也共用
/// `database::model::property_ref_of`。本函数只负责本屏关心的两件事：
/// 哪些类别**上屏**、行主文本怎么写。
///
/// **上屏范围**：除 `Catalog` 外全部可寻址类别（表 / 视图 / 列 / 模式 / 例程 / 序列 / 触发器）。
/// 例程这三类曾经被白名单挡掉（当时以为“暂无定位能力”），但属性面板早已支持
/// （`property_panel::load_properties` 有 Routine / Sequence / Trigger 三支），
/// 而 `#` 全文档档的 FTS 索引**本就会写入例程**——挡住它们等于“搜到了却什么也不显示”。
pub fn meta_object(hit: &database::nav_jobs::SearchHit) -> Option<MetaObject> {
    let object = engine::ObjectRef::from_index_hit(
        &hit.conn_id,
        &hit.object_type,
        &hit.object_name,
        hit.parent_name.as_deref(),
        hit.catalog.as_deref(),
        hit.schema.as_deref(),
    )?;
    let kind = match object.kind {
        engine::ObjectKind::Table => RowKind::Table,
        engine::ObjectKind::View => RowKind::View,
        engine::ObjectKind::Column => RowKind::Column,
        engine::ObjectKind::Schema => RowKind::Schema,
        engine::ObjectKind::Routine => RowKind::Routine,
        engine::ObjectKind::Sequence => RowKind::Sequence,
        engine::ObjectKind::Trigger => RowKind::Trigger,
        // Catalog 不立行：它不是“对象详情”的入口（导航里点库得到的是列表），
        // 且索引 / FTS 两侧都不写 catalog 行。
        engine::ObjectKind::Catalog => return None,
    };
    let title = match kind {
        RowKind::Column | RowKind::Trigger => match hit.parent_name.as_deref() {
            Some(parent) if !parent.is_empty() => format!("{parent}.{}", hit.object_name),
            _ => hit.object_name.clone(),
        },
        _ => match hit.schema.as_deref() {
            Some(schema) if !schema.is_empty() => format!("{schema}.{}", hit.object_name),
            _ => hit.object_name.clone(),
        },
    };
    Some(MetaObject {
        request: database::model::PropertyRequest {
            property: database::model::property_ref_of(&object),
            conn_label: hit.conn_label.clone(),
            driver: hit.driver.clone(),
            type_id: None,
            data: None,
        },
        object,
        title,
        kind,
        snippet: hit.snippet.clone(),
    })
}

/// 草稿箱扁平行 → 行数据（跨 crate 映射的唯一一处，字段同名不同源，便于对照）。
///
/// 为什么不让浮层直接拿着 `scratchpad::FlatFile`：`model` 是纯数据层（可单测、不认识宿主），
/// 映射集中在这里，将来换文件源（如资产库存档）只需再写一个这样的函数。
pub fn file_object(file: &scratchpad::FlatFile) -> FileObject {
    FileObject {
        relative_path: file.relative_path.clone(),
        folder: file.folder.clone(),
        name: file.name.clone(),
        path: file.path.clone(),
    }
}

/// 连接行（下标即 `Shared::selected` 的位置）。
pub(crate) fn connection_rows(connections: &[(String, String)]) -> Vec<Row> {
    connections
        .iter()
        .enumerate()
        .map(|(ix, (name, driver))| Row {
            key: format!("conn:{ix}:{name}"),
            kind: RowKind::Connection,
            title: name.clone(),
            match_text: name.clone(),
            secondary: driver.clone(),
            snippet: None,
            why: None,
            locate: None,
            action: Action::SelectConnection(ix),
            secondary_action: None,
        })
        .collect()
}
