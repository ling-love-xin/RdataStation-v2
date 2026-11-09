//! 分组装配：三源行映射 / 组头与截断 / 命令目录。

use crate::model::EditorMode;
use crate::quick_open::commands::{command_rows, command_specs};
use crate::quick_open::model::{
    Action, MetaObject, RowKind, Sources, build_groups, filter_ranked, match_span, meta_object,
    parse,
};
use property::PropertyKind;

use super::{conns, draft, hit};

#[test]
fn build_groups_filters_and_drops_empty_groups() {
    let conns = conns();
    // 命令模式：只有命令组
    let cmd = build_groups(
        &parse(">设置"),
        &Sources {
            connections: &conns,
            ..Default::default()
        },
    );
    assert_eq!(cmd.len(), 1);
    assert_eq!(cmd[0].title, "命令");
    assert!(cmd[0].rows.iter().all(|r| r.title.contains("设置")));

    // 默认模式：命中连接则不出现命令组
    let hit_conn = build_groups(
        &parse("分析库"),
        &Sources {
            connections: &conns,
            ..Default::default()
        },
    );
    assert_eq!(hit_conn.len(), 1);
    assert_eq!(hit_conn[0].title, "连接");
    assert_eq!(hit_conn[0].rows[0].action, Action::SelectConnection(1));

    // `#` 内容档：没有回填命中时为空（空态由委托给说明文案）
    assert!(
        build_groups(
            &parse("#渠道与"),
            &Sources {
                connections: &conns,
                ..Default::default()
            }
        )
        .is_empty()
    );

    // 无匹配：一个组都没有
    assert!(
        build_groups(
            &parse("zzzz"),
            &Sources {
                connections: &conns,
                ..Default::default()
            }
        )
        .is_empty()
    );
}

/// 文件源：相对路径是业务键与匹配面，主文本是文件名，动作带可打开的绝对路径。
#[test]
fn file_rows_match_on_name_and_path_but_highlight_the_name() {
    let files = [draft("notes/a.md"), draft("queries/last.sql")];
    let groups = build_groups(
        &parse("last"),
        &Sources {
            files: &files,
            ..Default::default()
        },
    );
    // 文件排在元数据之后、连接之前（原型设计 §7.2）；这里没有元数据与连接，它就是第一组
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].title, "文件（草稿箱）");
    let row = &groups[0].rows[0];
    assert_eq!(row.title, "last.sql", "主文本是文件名");
    assert_eq!(row.secondary, "queries", "右侧是所在目录");
    assert_eq!(row.key, "file:queries/last.sql", "键是相对路径");
    assert_eq!(row.kind.label(), "文件");
    match &row.action {
        Action::OpenDocument(path) => {
            assert!(
                path.ends_with("queries/last.sql"),
                "动作要带绝对路径：{path:?}"
            )
        }
        other => panic!("文件行动作应是打开文档，实际 {other:?}"),
    }

    // 按**目录**也能搜到（匹配面是相对路径），但高亮仍按文件名算（文件名里没有 `notes`）
    let by_folder = build_groups(
        &parse("notes"),
        &Sources {
            files: &files,
            ..Default::default()
        },
    );
    assert_eq!(by_folder[0].rows.len(), 1);
    assert!(match_span(&by_folder[0].rows[0].title, "notes").is_none());

    // 清单被后台截断：组头要说「没全列」而不是安静地少几行
    let note = build_groups(
        &parse("a"),
        &Sources {
            files: &files,
            files_note: "文件较多，只列前 500 个（继续输入缩小范围）".to_string(),
            ..Default::default()
        },
    );
    assert!(
        note[0].note.contains("只列前"),
        "截断要说清：{}",
        note[0].note
    );

    // 没打开项目（文件源为空）→ 连空组都不出现
    assert!(build_groups(&parse("last"), &Sources::default()).is_empty());
}

#[test]
fn meta_rows_come_first_and_carry_properties_request() {
    let conns = conns();
    let objects: Vec<MetaObject> = [
        hit("table", "orders", None),
        hit("column", "order_id", Some("orders")),
        hit("routine", "fn_x", None), // 例程也立行（属性面板支持；FTS 会命中它）
        hit("index", "idx_a", None),  // 不可寻址类别（索引）→ 不立行
    ]
    .iter()
    .filter_map(meta_object)
    .collect();
    assert_eq!(objects.len(), 3, "索引行应被过滤掉，例程应保留");

    let groups = build_groups(
        &parse("order"),
        &Sources {
            connections: &conns,
            meta: &objects,
            ..Default::default()
        },
    );
    // 元数据排在连接 / 命令之前
    assert_eq!(
        groups[0].title,
        "元数据（表 · 视图 · 列 · 例程 · 序列 · 触发器 · 模式）"
    );
    assert_eq!(groups[0].rows.len(), 2, "`order` 只命中表与列两行");
    // 表行标题是 `schema.name`，列行是 `父表.列`
    assert!(groups[0].rows.iter().any(|r| r.title == "public.orders"));
    assert!(groups[0].rows.iter().any(|r| r.title == "orders.order_id"));
    // 列行的业务键带父表（否则同名列会撞键）
    let col = groups[0]
        .rows
        .iter()
        .find(|r| r.kind == RowKind::Column)
        .expect("列行");
    assert!(col.key.contains("orders"), "列键要含父表：{}", col.key);
    match &col.action {
        Action::ShowProperties(request) => {
            assert_eq!(request.property.parent.as_deref(), Some("orders"));
            assert_eq!(request.property.conn_id, "P_conn");
            assert_eq!(request.conn_label, "营销分析（生产）");
        }
        other => panic!("列行动作应是属性面板，实际 {other:?}"),
    }
    // 同一行还带「在树中定位」的引用（Quick Open 的 ⌥↵）：列必须带父表，
    // 否则树里挂在表下那一层找不到它（定位会停在文件夹那一层）。
    let locate = col.locate.as_ref().expect("元数据行应带定位引用");
    assert_eq!(locate.parent, "orders", "列的定位引用要带父表");
    assert_eq!(locate.name, "order_id");
    assert_eq!(locate.schema, "public");
    // 表行同样可定位（不带父对象）
    let table = groups[0]
        .rows
        .iter()
        .find(|r| r.kind == RowKind::Table)
        .expect("表行");
    let table_locate = table.locate.as_ref().expect("表行应可定位");
    assert!(table_locate.parent.is_empty(), "表没有父对象");

    // 例程行：类目标签与属性类别都要对（曾经被白名单挡掉）
    let routine = objects
        .iter()
        .find(|m| m.kind == RowKind::Routine)
        .expect("例程应立行");
    assert_eq!(routine.request.property.kind, PropertyKind::Routine);
    assert_eq!(routine.kind.label(), "例程");
}

#[test]
fn searching_note_shows_only_when_query_is_ready() {
    let conns = conns();
    let objects: Vec<MetaObject> = [hit("table", "orders", None)]
        .iter()
        .filter_map(meta_object)
        .collect();
    // 搜索中 + 词长够 → 组头带「搜索中…」
    let busy = build_groups(
        &parse("or"),
        &Sources {
            connections: &conns,
            meta: &objects,
            meta_searching: true,
            ..Default::default()
        },
    );
    assert_eq!(busy[0].note, "搜索中…");
    // 单字符（未达门槛）→ 不发搜索，也不显示搜索中
    let short = build_groups(
        &parse("o"),
        &Sources {
            connections: &conns,
            meta: &objects,
            meta_searching: true,
            ..Default::default()
        },
    );
    assert!(short.iter().all(|g| g.note.is_empty()));
}

/// 内容档行：只在注释里命中的对象**不被标题过滤误杀**，并标「内容」/「名称」。
#[test]
fn fulltext_rows_keep_content_hits_and_tag_the_reason() {
    let mut content_hit = hit("table", "orders", None);
    content_hit.snippet = Some("…含<mark>渠道</mark>与优惠…".to_string());
    let content = meta_object(&content_hit).expect("表命中");
    let rows = build_groups(
        &parse("#含渠道"),
        &Sources {
            meta: &[content],
            ..Default::default()
        },
    );
    assert_eq!(rows[0].title, "元数据全文（注释）");
    let row = &rows[0].rows[0];
    assert_eq!(row.title, "public.orders");
    assert!(
        row.snippet
            .as_deref()
            .unwrap_or_default()
            .contains("<mark>"),
        "snippet 要带命中标记（UI 靠它上色）"
    );
    assert_eq!(row.why, Some("内容"), "标题不含词 → 内容命中");

    // 标题也含词 → 标「名称」
    let mut name_hit = hit("table", "orders", None);
    name_hit.snippet = Some("<mark>ord</mark>ers".to_string());
    let name = meta_object(&name_hit).expect("表命中");
    let rows = build_groups(
        &parse("#ord"),
        &Sources {
            meta: &[name],
            ..Default::default()
        },
    );
    assert_eq!(rows[0].rows[0].why, Some("名称"));
}

/// 渲染上限：单组 ≤ 8 行、总计 ≤ 50 行；被截的条数落在 `hidden` 上（组尾显示）。
#[test]
fn groups_are_capped_and_report_what_was_hidden() {
    let many: Vec<(String, String)> = (0..20)
        .map(|i| (format!("conn{i:02}"), "postgres".to_string()))
        .collect();
    let groups = build_groups(
        &parse(""),
        &Sources {
            connections: &many,
            ..Default::default()
        },
    );
    let conns = groups.iter().find(|g| g.title == "连接").expect("连接组");
    assert_eq!(conns.rows.len(), 8, "单组上限 8 行");
    assert_eq!(conns.hidden, 12, "被截的条数要记下来（组尾显示）");

    // 总量上限：命令档只有一组，构造一个命中很多的词也超不过 50；
    // 这里直接验证「总计不超过 50」这条不变量。
    let total: usize = groups.iter().map(|g| g.rows.len()).sum();
    assert!(total <= 50, "总渲染量不得超过 50 行：{total}");
}

/// 命令的英文别名（`keywords`）参与匹配，但高亮仍按展示名算。
///
/// 这层是英文 / 拼音输入习惯的兼容：中文用户敲 `settings` / `database`
/// 也能找到对应条目（且命中不靠“给每条命令记英文展示名”）。
#[test]
fn command_keywords_match_without_painting_highlights_on_the_label() {
    let groups = build_groups(&parse(">settings"), &Sources::default());
    assert_eq!(groups.len(), 1, "命令档只出命令组");
    assert_eq!(groups[0].rows.len(), 1, "`settings` 只应命中打开设置");
    let row = &groups[0].rows[0];
    assert_eq!(row.title, "打开设置");
    assert_eq!(row.key, "cmd:app.settings", "键跟着稳定 id 走");
    // 展示名里没有 `settings` → 不该算出高亮区间（否则会标出莫名其妙的黄块）
    assert!(match_span(&row.title, "settings").is_none());

    let groups = build_groups(&parse(">database"), &Sources::default());
    let hit = groups[0]
        .rows
        .iter()
        .find(|r| r.title == "打开数据库导航")
        .expect("`database` 应命中数据库导航");
    assert_eq!(hit.key, "cmd:panel.database");

    // 中文输入照旧按展示名命中（关键词不抢展示名的主位）
    let groups = build_groups(&parse(">设置"), &Sources::default());
    assert_eq!(groups[0].rows[0].title, "打开设置");

    // 单字符不进关键词匹配面：否则 `s` 会把一半命令都捞出来（噪声）
    let groups = build_groups(&parse(">s"), &Sources::default());
    assert!(
        groups.is_empty(),
        "单字符不应命中英文关键词：{:?}",
        groups.iter().map(|g| g.title).collect::<Vec<_>>()
    );
}

/// 命令目录的 id 与行键（`cmd:{id}`）都要稳定且唯一。
///
/// `id` 是选中跟随的键，也是将来跨 crate 登记表的键：`label` 可以改、`id` 不能改
/// （改了会让「记住了上次选中的命令」这类能力失效）。
#[test]
fn command_ids_and_row_keys_are_stable_and_unique() {
    let specs = command_specs();
    let mut ids: Vec<&str> = specs.iter().map(|s| s.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "命令 id 必须唯一");

    let rows = command_rows();
    assert_eq!(rows.len(), total, "每条命令都要出一行");
    let mut keys: Vec<&String> = rows.iter().map(|r| &r.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), total, "命令键必须唯一（选中跟随依赖它）");
    assert_eq!(rows[0].action, Action::NewDocument(EditorMode::Sql));
    // 关键词不能漏在匹配面之外：目录里写了就必须能被搜到
    for spec in specs {
        assert!(
            spec.keywords.trim().is_empty()
                || spec.keywords.split_whitespace().all(|kw| {
                    filter_ranked(command_rows(), kw)
                        .iter()
                        .any(|r| r.key == format!("cmd:{}", spec.id))
                }),
            "命令 {} 的 keywords 应当个个都能命中自己",
            spec.id
        );
    }
}
