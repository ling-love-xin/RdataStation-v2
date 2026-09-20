//! 离线性能基线（无 UI / 可重复 / 带阈值断言）
//!
//! ## 这份文件回答什么
//!
//! 「改了这里会不会变慢」此前只能靠开着窗口敲字去感觉。本文件**不启动 GPUI、不开窗口**，
//! 把编辑路径上的**纯函数**跑在固定输入上：先热身再采样，给 p50 / p95 / p99，
//! 最后对 p95 下**宽松但能抓住数量级退化**的阈值——回归的标准不是「慢了 10% 就红」，
//! 而是「多扫了一遍全文」「少了一层门槛」才红。
//!
//! ## 覆盖的真实调用点
//!
//! | 用例 | 真实调用点（谁在什么时机调） |
//! | --- | --- |
//! | `fold::spans` | `view/host.rs`：每次 `InputEvent::Change`（即每次按键）重算折叠候选 |
//! | 语句切分 | `view/host.rs::count_statements`（状态栏语句数，每次按键）+ 执行 / 批量目标 |
//! | `highlight_spans` | `view/highlight.rs::tokens_for`：每次语义 token 请求（可视区间）全文扫一遍 |
//! | 补全候选 | `view/completion.rs`：打字触发的补全弹层，每次按键一次（`MAX_ITEMS = 60`） |
//! | 导出编码 | `export::encode`：CSV / JSON / INSERT 的纯编码路径（1 万行 × 10 列） |
//!
//! ## 运行约定（稳定性）
//!
//! - **先热身再采样**：首次调用要付分配器 / 分支预测 / 惰性初始化的账，混进样本会让 p50 偏高、
//!   p99 失真——热身轮数写在每个用例的调用里，且**不计入样本**；
//! - **样本数写在打印里**：慢的用例样本少（每次几十毫秒），快的用例样本多（每次几微秒）；
//! - **阈值 = 本机 debug 实测 p95 的 4~5 倍**（O(1) 那几条另加绝对下限），所以只有数量级退化会触发；
//!   阈值是跟着实测走的，改完实现请重跑并重标（每条阈值的来历写在它自己的 `why` 字符串里）；
//! - 本文件的用例**串行**跑（`exclusive()`）：并发会在同一批核上互抢，p95 立刻失真；
//! - 数字随机器 / 负载 / 工具链而变：**只在同一台机器上前后对比**才有意义，
//!   不要拿这里的绝对值当跨机器的性能指标。
//!
//! ## 边界
//!
//! 只覆盖**纯计算**：I/O、渲染、GPUI 事件循环不在范围内（那些要真窗口）。
//!
//! ```sh
//! cargo test -p rds-editor --test offline_baseline -j 2 -- --nocapture
//! ```

use std::hint::black_box;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use engine::sql::{highlight_spans, split_statements};
use rds_editor::completion::{Candidate, CandidateKind, Catalog, Request, candidates};
use rds_editor::export::{self, ExportFormat};
use rds_editor::fold;
use rds_editor::model::DocumentId;
use rds_editor::store::ResultEntry;

// ═══════════════════════════════════════════════════════════════════
// 统计骨架（不引 criterion：只用 std::time::Instant 自己采）
// ═══════════════════════════════════════════════════════════════════

/// 用例之间互斥（见文件头「运行约定」）
static EXCLUSIVE: Mutex<()> = Mutex::new(());

fn exclusive() -> MutexGuard<'static, ()> {
    // 前一个用例断言失败会让锁中毒；这里不把 panic 传染给后续用例——后面的数字照样要打出来
    EXCLUSIVE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 一条用例的统计量（毫秒）
#[derive(Clone)]
struct Stats {
    name: String,
    samples: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    /// 折算出来的单位成本（「每条语句 / 每行」各花多少）与它的名字
    unit: Option<(&'static str, f64)>,
}

impl Stats {
    fn from_samples(name: impl Into<String>, mut took: Vec<f64>) -> Self {
        assert!(!took.is_empty(), "样本不能为空");
        took.sort_by(f64::total_cmp);
        // 最近秩法：打印出来的就是样本里的**真实值**，不是插值出来的中间数
        let at = |p: f64| -> f64 {
            let rank = (p * took.len() as f64).ceil() as usize;
            took[rank.clamp(1, took.len()) - 1]
        };
        Self {
            name: name.into(),
            samples: took.len(),
            p50: at(0.50),
            p95: at(0.95),
            p99: at(0.99),
            max: *took.last().expect("非空"),
            unit: None,
        }
    }

    /// 附上折算成本（如「每条语句」的平均耗时）：按 p50 折算，不给噪声放大
    fn with_unit(mut self, label: &'static str, divisor: f64) -> Self {
        self.unit = Some((label, self.p50 / divisor));
        self
    }
}

/// 采样：先热身 `warmup` 轮（不计入样本），再连采 `samples` 次
fn measure<T>(
    name: impl Into<String>,
    warmup: usize,
    samples: usize,
    mut body: impl FnMut() -> T,
) -> Stats {
    for _ in 0..warmup {
        black_box(body());
    }
    let mut took = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let value = body();
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        // 结果吃掉：算出来的东西不能被优化掉（debug 下无所谓，release 下这是必要条件）
        black_box(&value);
        took.push(elapsed);
    }
    Stats::from_samples(name, took)
}

/// 一份基线报告：先把所有用例采完并打印，最后统一断言
/// （一条失败不影响其余数字被看到——排障时要的是全貌）
#[derive(Default)]
struct Report {
    title: &'static str,
    notes: Vec<String>,
    rows: Vec<Row>,
}

struct Row {
    stats: Stats,
    /// p95 阈值（毫秒）
    budget: f64,
    /// 这条阈值是怎么定的（断言失败时打出来）
    why: &'static str,
}

impl Report {
    fn new(title: &'static str) -> Self {
        Self {
            title,
            notes: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// 输入规模一类的背景信息（打在表格上方，让数字自解释）
    fn note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }

    /// 收一条用例：`budget` 是 p95 阈值（毫秒），返回统计量供再做相对断言
    fn record(&mut self, stats: Stats, budget: f64, why: &'static str) -> Stats {
        self.rows.push(Row {
            stats: stats.clone(),
            budget,
            why,
        });
        stats
    }

    /// 打印全表 + 逐条断言 p95
    fn finish(self) {
        const HEAD: &str = "   用例";
        println!("\n══ {} ══", self.title);
        println!(
            "   构建档位：{}（阈值按 debug 实测标定；release 只会更快）",
            if cfg!(debug_assertions) {
                "debug（未优化）"
            } else {
                "release"
            }
        );
        for note in &self.notes {
            println!("   {note}");
        }
        println!(
            "{}{}",
            pad(HEAD, 38),
            format!(
                "{:>7}{:>13}{:>13}{:>13}{:>13}{:>16}",
                "样本", "p50", "p95", "p99", "max", "阈值(p95)"
            )
        );
        for row in &self.rows {
            let stats = &row.stats;
            let numbers = format!(
                "{:>7}{:>13}{:>13}{:>13}{:>13}{:>16}",
                stats.samples,
                human_ms(stats.p50),
                human_ms(stats.p95),
                human_ms(stats.p99),
                human_ms(stats.max),
                format!("< {}", human_ms(row.budget)),
            );
            println!("{}{numbers}", pad(&format!("   {}", stats.name), 38));
        }
        for row in &self.rows {
            if let Some((label, cost)) = row.stats.unit {
                println!(
                    "   · {}：平均 {} / {label}（按 p50 折算）",
                    row.stats.name,
                    human_ms(cost)
                );
            }
        }
        println!(
            "   （< 0.010ms 的按 µs 显示；数字随机器 / 负载而变，只在同一台机器上做前后对比）"
        );

        let failures: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.stats.p95 > row.budget)
            .map(|row| {
                format!(
                    "{}：p95 {} 超过阈值 {}（相差 {:.1} 倍，样本 {}）—— {}",
                    row.stats.name,
                    human_ms(row.stats.p95),
                    human_ms(row.budget),
                    row.stats.p95 / row.budget,
                    row.stats.samples,
                    row.why,
                )
            })
            .collect();
        assert!(
            failures.is_empty(),
            "离线基线退化（数量级变了才该出现这条）：\n   {}",
            failures.join("\n   ")
        );
    }
}

/// 毫秒 → 人看的字符串（小于 0.01ms 换成 µs，免得一列 `0.0003` 看不出量级）
fn human_ms(ms: f64) -> String {
    if ms < 0.01 {
        format!("{:.3}µs", ms * 1e3)
    } else {
        format!("{:.4}ms", ms)
    }
}

/// 按**显示宽度**补齐（CJK 算两格）：否则中文用例名会把表头挤歪
fn pad(text: &str, width: usize) -> String {
    let shown: usize = text
        .chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum();
    format!("{text}{}", " ".repeat(width.saturating_sub(shown)))
}

fn kb(text: &str) -> String {
    format!("{:.1} KB", text.len() as f64 / 1024.0)
}

// ═══════════════════════════════════════════════════════════════════
// 测试数据（生成成本不计入样本）
// ═══════════════════════════════════════════════════════════════════

/// 一个「像真实脚本」的单元：12 行，含一个多行括号组、一个 `CASE … END`、一段多行块注释
/// ——正是 `fold::spans` 认的三类结构，每单元应当产出 3 个候选
const UNIT_LINES: usize = 12;

fn script_unit(index: usize, out: &mut String) {
    out.push_str(&format!("-- 订单汇总 {index}\n"));
    out.push_str("SELECT o.id,\n");
    out.push_str("       CASE\n");
    out.push_str("           WHEN o.status = 'paid' THEN o.total\n");
    out.push_str("           ELSE 0\n");
    out.push_str("       END AS paid_total,\n");
    out.push_str("       (\n");
    out.push_str("           SELECT count(*) FROM order_items i WHERE i.order_id = o.id\n");
    out.push_str("       ) AS item_count\n");
    out.push_str("/* 历史口径说明（含括号 ( 与分号 ;）\n");
    out.push_str("   第二行注释 */\n");
    out.push_str("FROM orders o;\n");
}

fn script_units(units: usize) -> String {
    let mut text = String::with_capacity(units * 400);
    for index in 0..units {
        script_unit(index, &mut text);
    }
    text
}

/// 约 `lines` 行的脚本（按单元向上取整）
fn script_lines(lines: usize) -> String {
    script_units(lines.div_ceil(UNIT_LINES))
}

/// 至少 `bytes` 字节的脚本（整单元拼到够为止——单元不能切一半，否则词法层看到的是半截结构）
fn script_bytes(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes + 1_024);
    let mut index = 0;
    while text.len() < bytes {
        script_unit(index, &mut text);
        index += 1;
    }
    text
}

fn units_of(text: &str) -> usize {
    text.lines().count() / UNIT_LINES
}

/// 一批「一行一条」的语句，三种形态轮着来（贴近真实脚本的词汇分布：
/// 字符串里的分号、括号、数字、中文注释行）
fn statements_fn(count: usize) -> String {
    let mut text = String::with_capacity(count * 72);
    for index in 0..count {
        match index % 3 {
            0 => text.push_str(&format!(
                "SELECT id, name FROM users WHERE id = {index} AND note = 'a;b{index}';\n"
            )),
            1 => text.push_str(&format!(
                "INSERT INTO orders (id, total) VALUES ({index}, {index}.5);\n"
            )),
            _ => text.push_str(&format!(
                "UPDATE users SET name = 'n{index}' WHERE id = {index}; -- 第 {index} 条\n"
            )),
        }
    }
    text
}

/// ~2 千条目的假目录（200 张表 × 9 列 + 200 个表 + 20 个视图）——真实宿主给的
/// 就是「这条连接上所有 schema / 表 / 列」的一次快照（`workbench/src/services/editor_completion.rs`）
fn fake_catalog() -> Catalog {
    const COLUMNS: [&str; 9] = [
        "id",
        "customer_id",
        "status",
        "total",
        "currency",
        "created_at",
        "updated_at",
        "note",
        "channel",
    ];
    const DETAILS: [&str; 5] = [
        "bigint",
        "varchar(64)",
        "numeric(12,2)",
        "timestamptz",
        "text",
    ];

    let mut catalog = Catalog::default();
    for index in 0..200 {
        let schema = if index % 2 == 0 { "public" } else { "sales" };
        let table = format!("{schema}.orders_{index:04}");
        catalog.objects.push(
            Candidate::new(table.clone(), CandidateKind::Table).with_detail("订单表（月度分区）"),
        );
        for (col, name) in COLUMNS.iter().enumerate() {
            catalog.columns.push((
                table.clone(),
                (*name).to_string(),
                Some(DETAILS[col % DETAILS.len()].to_string()),
            ));
        }
    }
    for index in 0..20 {
        catalog.objects.push(
            Candidate::new(format!("public.v_orders_{index:04}"), CandidateKind::View)
                .with_detail("订单视图"),
        );
    }
    catalog
}

/// 1 万行 × 10 列的假结果集（值按展示文本；含逗号 / 引号 / 中文 / NULL 这些要转义的形态）
fn export_entry(rows: usize, columns: usize) -> ResultEntry {
    const NAMES: [&str; 10] = [
        "id",
        "user_id",
        "status",
        "total",
        "currency",
        "created_at",
        "note",
        "city",
        "deleted_at",
        "extra",
    ];
    let names: Vec<String> = NAMES
        .iter()
        .take(columns)
        .map(|n| (*n).to_string())
        .collect();
    let data: Vec<Vec<String>> = (0..rows)
        .map(|row| (0..columns).map(|col| export_cell(col, row)).collect())
        .collect();
    ResultEntry::success(
        DocumentId::new("offline-baseline"),
        "SELECT * FROM public.orders".to_string(),
        0,
        false,
        names,
        data,
    )
}

fn export_cell(col: usize, row: usize) -> String {
    match col {
        0 => row.to_string(),
        1 => (100_000 + row).to_string(),
        2 => ["paid", "shipped", "pending", "cancelled"][row % 4].to_string(),
        3 => format!("{row}.25"),
        4 => ["USD", "CNY", "EUR"][row % 3].to_string(),
        5 => format!("2026-09-{:02} 12:00:00", row % 28 + 1),
        // 每 10 行来一个 CSV / JSON 都要转义的值
        6 if row % 10 == 0 => format!("备注,含逗号与\"引号\" {row}"),
        7 => ["北京", "上海", "深圳", "广州"][row % 4].to_string(),
        // 每 7 行一个 NULL（网格里的斜体那一档）
        8 if row % 7 == 0 => "NULL".to_string(),
        _ => format!("note_{row}"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 一、折叠候选（`fold::spans`）
// ═══════════════════════════════════════════════════════════════════

/// `fold::spans`：小脚本 / 5 千行 / 贴着门槛 / 超门槛
///
/// 真实时机：`view/host.rs` 每次 `InputEvent::Change` 都重算一次（每次按键）。
#[test]
fn fold_candidates_baseline() {
    let _guard = exclusive();
    let mut report = Report::new("折叠候选 editor::fold::spans（每次按键重算）");

    let small = script_lines(100);
    let medium = script_lines(5_000);
    let at_gate = script_bytes(970_000);
    let over_gate = script_bytes(fold::MAX_BYTES + 10_000);

    report.note(format!(
        "输入：{} 行 {} / {} 行 {} / 贴合门槛 {} / 超门槛 {}（fold::MAX_BYTES = {} 字节）",
        small.lines().count(),
        kb(&small),
        medium.lines().count(),
        kb(&medium),
        kb(&at_gate),
        kb(&over_gate),
        fold::MAX_BYTES,
    ));

    // 先确认测的是真东西：每个 12 行单元产 3 个候选（CASE 块 / 括号组 / 多行注释各一）
    for (label, text) in [
        ("100 行", &small),
        ("5 千行", &medium),
        ("贴合门槛", &at_gate),
    ] {
        assert_eq!(
            fold::spans(text).len(),
            3 * units_of(text),
            "{label} 的候选数不对：候选算空的话这条基线就没有意义了"
        );
    }
    // 门槛两侧：同样的内容，门槛内**有**候选、超门槛**没有**——空结果只可能来自那道门
    assert!(
        at_gate.len() <= fold::MAX_BYTES && !fold::spans(&at_gate).is_empty(),
        "门槛内必须有候选"
    );
    assert!(
        over_gate.len() > fold::MAX_BYTES,
        "超门槛的输入要真的超门槛"
    );
    assert!(
        fold::spans(&over_gate).is_empty(),
        "超门槛必须是「直接返回空」——这是把它压住的机制本身"
    );

    let small_stats = report.record(
        measure("折叠候选 · 100 行小脚本", 20, 200, || {
            fold::spans(&small).len()
        }),
        8.0,
        "实测 p95 1.1~1.6ms（多次运行）；8ms ≈ 5 倍余量，只有「小文本也开始全量重扫」这类数量级退化才触发",
    );
    let medium_stats = report.record(
        measure(
            format!("折叠候选 · 5 千行（{}）", kb(&medium)),
            3,
            25,
            || fold::spans(&medium).len(),
        ),
        350.0,
        "实测 p95 46~71ms（多次运行）；350ms ≈ 5 倍余量，只抓「多扫一遍 / 换更慢的词法器」",
    );
    let gate_stats = report.record(
        measure(
            format!("折叠候选 · 贴着门槛（{}）", kb(&at_gate)),
            1,
            10,
            || fold::spans(&at_gate).len(),
        ),
        2_500.0,
        "门槛内最坏情况（全文词法 + 配对 + 排序）：实测 p95 338~508ms；2500ms ≈ 5 倍余量",
    );
    let over_stats = report.record(
        measure("折叠候选 · 超门槛（直接返回空）", 50, 1_000, || {
            fold::spans(&over_gate).len()
        }),
        0.05,
        "O(1) 早退（只比长度）：实测 p95 0.1µs，这里给的是**绝对下限** 50µs——它要抓的是「门槛没了 → 全文扫 300ms 量级」",
    );

    // 门槛是**成本机制**：超门槛那次必须是 O(1)，与全长扫描差着两个数量级以上
    assert!(
        over_stats.p95 * 20.0 < gate_stats.p95,
        "超门槛没有变便宜（over p95 {} vs 贴合门槛 p95 {}）：早退的门槛可能被绕过了",
        human_ms(over_stats.p95),
        human_ms(gate_stats.p95),
    );
    // 相对关系也钉一下：小文本必须显著便宜于 5 千行（否则说明规模门槛 / 缓存出了问题）
    assert!(
        small_stats.p95 < medium_stats.p95,
        "100 行比 5 千行还慢（{} vs {}）：数据或实现有问题",
        human_ms(small_stats.p95),
        human_ms(medium_stats.p95),
    );

    report.finish();
}

// ═══════════════════════════════════════════════════════════════════
// 二、语句切分（`engine::sql::split_statements`）
// ═══════════════════════════════════════════════════════════════════

/// 语句切分：1 千条 / 1 万条，并折算「每条语句平均成本」
///
/// 为什么盯折算值：状态栏的语句数（`count_statements`）与折叠都在**全文扫**，
/// 而状态栏那条是每次按键都跑一次——「每条语句多少微秒」才换算得出「1 万行的脚本敲一下要等多久」。
#[test]
fn statement_splitting_baseline() {
    let _guard = exclusive();
    let mut report =
        Report::new("语句切分 engine::sql::split_statements（状态栏每次按键 / 执行 / 批量）");

    let thousand = statements_fn(1_000);
    let ten_thousand = statements_fn(10_000);

    report.note(format!(
        "输入：1 千条 {} / 1 万条 {}",
        kb(&thousand),
        kb(&ten_thousand)
    ));

    // 测的是真东西：条数对上（切空 / 切错都会在这里先暴露）
    assert_eq!(split_statements(&thousand).len(), 1_000);
    assert_eq!(split_statements(&ten_thousand).len(), 10_000);

    report.record(
        measure("语句切分 · 1 千条", 10, 100, || {
            split_statements(&thousand).len()
        })
        .with_unit("每条语句", 1_000.0),
        60.0,
        "实测 p95 3.5~13.4ms（机器有并发 cargo 时抛尾明显）；60ms ≈ 4.5 倍余量，只抓数量级退化",
    );
    report.record(
        measure("语句切分 · 1 万条", 3, 30, || {
            split_statements(&ten_thousand).len()
        })
        .with_unit("每条语句", 10_000.0),
        250.0,
        "实测 p95 约 55ms（约 3.9µs/条）；250ms ≈ 4.5 倍余量（这条是状态栏每次按键都跑的那条路）",
    );

    report.finish();
}

// ═══════════════════════════════════════════════════════════════════
// 三、词法高亮（`engine::sql::highlight_spans`）
// ═══════════════════════════════════════════════════════════════════

/// `view/highlight.rs` 的着色门槛（`HIGHLIGHT_MAX_BYTES`，那是个私有常量，这里按同值断言）
const HIGHLIGHT_GATE: usize = 1_000_000;

/// 词法高亮：5 千行 / 贴着 1 MB 门槛（**最坏情况**）
///
/// 真实时机：`view/highlight.rs::tokens_for` 每次语义 token 请求都会
/// `text.to_string()` + `highlight_spans(全文)`，再按可视区间筛——所以「一个可视区间」
/// 的成本其实是**整篇的词法扫描**。这条基线量的就是它。
#[test]
fn highlight_spans_baseline() {
    let _guard = exclusive();
    let mut report = Report::new("词法高亮 engine::sql::highlight_spans（每次可视区间请求全文扫）");

    let medium = script_lines(5_000);
    let at_gate = script_bytes(970_000);

    report.note(format!(
        "输入：{} 行 {} / 贴着门槛 {}（view 层的门槛 {} 字节）",
        medium.lines().count(),
        kb(&medium),
        kb(&at_gate),
        HIGHLIGHT_GATE,
    ));

    // 测的是真东西：区间数合理（不是早退回空），且门槛内那一份确实还没被门槛拦住
    let medium_spans = highlight_spans(&medium).len();
    let gate_spans = highlight_spans(&at_gate).len();
    assert!(
        medium_spans > 10_000,
        "5 千行脚本的高亮区间太少（{medium_spans}）：词法可能没跑起来"
    );
    assert!(gate_spans > medium_spans, "门槛内那一份应当更多区间");
    assert!(at_gate.len() < HIGHLIGHT_GATE, "贴着门槛的输入要在门槛之内");

    report.record(
        measure(
            format!("词法高亮 · 5 千行（{}）", kb(&medium)),
            3,
            25,
            || highlight_spans(&medium).len(),
        ),
        300.0,
        "实测 p95 49~60ms；300ms ≈ 5 倍余量",
    );
    report.record(
        measure(
            format!("词法高亮 · 贴着门槛（{}）", kb(&at_gate)),
            1,
            10,
            || highlight_spans(&at_gate).len(),
        ),
        2_200.0,
        "门槛内最坏情况：实测 p95 379~480ms；2200ms ≈ 4.6 倍余量（每次可视区间请求都要付这一笔）",
    );

    report.finish();
}

// ═══════════════════════════════════════════════════════════════════
// 四、补全候选挑排（`editor::completion::candidates`）
// ═══════════════════════════════════════════════════════════════════

/// 补全候选：~2 千条目目录下的五种请求形态
///
/// 真实时机：`view/completion.rs` 打字触发（每个标识符字符一次）与 `Ctrl+Space` 手动触发。
/// 上限是那里的 `MAX_ITEMS = 60`。
#[test]
fn completion_candidates_baseline() {
    const MAX_ITEMS: usize = 60;

    let _guard = exclusive();
    let mut report = Report::new("补全候选挑排 editor::completion::candidates（每次按键一次）");

    let catalog = fake_catalog();
    let entries = catalog.objects.len() + catalog.columns.len();

    let any_empty = Request::Any {
        prefix: String::new(),
    };
    let any_ord = Request::Any {
        prefix: "ord".to_string(),
    };
    let column_cust = Request::Column {
        prefix: "cust".to_string(),
    };
    let qualified_hit = Request::Qualified {
        qualifier: "public.orders_0002".to_string(),
        prefix: String::new(),
    };
    let qualified_miss = Request::Qualified {
        qualifier: "o".to_string(),
        prefix: String::new(),
    };
    let no_hit = Request::Any {
        prefix: "zzzz_没有这个东西".to_string(),
    };

    report.note(format!(
        "目录：{} 条目（{} 个表/视图 + {} 列）；上限 MAX_ITEMS = {MAX_ITEMS}",
        entries,
        catalog.objects.len(),
        catalog.columns.len(),
    ));
    assert!(
        (2_000..2_100).contains(&entries),
        "假目录要 ~2 千条目，实际 {entries}"
    );

    // 测的是真东西：每条路都真挑出了东西（空前缀那条正好顶到上限），没命中的那条给空
    assert_eq!(candidates(&catalog, &any_empty, MAX_ITEMS).len(), MAX_ITEMS);
    assert!(!candidates(&catalog, &any_ord, MAX_ITEMS).is_empty());
    assert!(!candidates(&catalog, &column_cust, MAX_ITEMS).is_empty());
    assert_eq!(
        candidates(&catalog, &qualified_hit, MAX_ITEMS).len(),
        9,
        "限定名命中一张表 → 那 9 列"
    );
    assert_eq!(
        candidates(&catalog, &qualified_miss, MAX_ITEMS).len(),
        9,
        "限定名认不出（表别名）→ 兜底给全部列；但 200 张表的列**同名**，去重后只剩 9 个名字"
    );
    assert!(
        candidates(&catalog, &no_hit, MAX_ITEMS).is_empty(),
        "无命中必须给空，不能全量兜底"
    );

    report.record(
        measure(
            "补全 · 空前缀 Any（Ctrl+Space 全量挑排）",
            10,
            100,
            || candidates(&catalog, &any_empty, MAX_ITEMS).len(),
        ),
        15.0,
        "实测 p95 约 3.2ms（debug，2020 条目全量排序）；15ms ≈ 4.7 倍余量（排序 / 去重变复杂会在这里现形）",
    );
    report.record(
        measure("补全 · 前缀 Any「ord」", 10, 100, || {
            candidates(&catalog, &any_ord, MAX_ITEMS).len()
        }),
        17.0,
        "实测 p95 约 3.8ms（debug，200 个表名全部前缀命中）；17ms ≈ 4.5 倍余量",
    );
    report.record(
        measure("补全 · 列位 Column「cust」", 10, 100, || {
            candidates(&catalog, &column_cust, MAX_ITEMS).len()
        }),
        13.0,
        "实测 p95 约 2.9ms（debug，1800 列全扫）；13ms ≈ 4.5 倍余量",
    );
    report.record(
        measure("补全 · 限定名命中（o.）", 10, 100, || {
            candidates(&catalog, &qualified_hit, MAX_ITEMS).len()
        }),
        15.0,
        "实测 p95 约 3.3ms（debug）；15ms ≈ 4.5 倍余量",
    );
    report.record(
        measure(
            "补全 · 限定名不命中（表别名兜底全列）",
            10,
            100,
            || candidates(&catalog, &qualified_miss, MAX_ITEMS).len(),
        ),
        25.0,
        "实测 p95 约 5.6ms（debug，最重的一条：1800 列全进候选再去重）；25ms ≈ 4.5 倍余量",
    );

    report.finish();
}

// ═══════════════════════════════════════════════════════════════════
// 五、导出编码（`editor::export`，CSV / JSON / INSERT）
// ═══════════════════════════════════════════════════════════════════

/// 导出编码：1 万行 × 10 列，三档文本格式
///
/// 纯编码路径（不落盘、不碰 DuckDB）：它只该是「一遍遍历 + 拼字符串」的成本。
#[test]
fn export_encoding_baseline() {
    const ROWS: usize = 10_000;
    const COLS: usize = 10;
    // 每 200 行一条 INSERT（`export.rs` 的 `INSERT_ROWS_PER_STATEMENT`）
    const INSERT_STATEMENTS: usize = ROWS.div_ceil(200);

    let _guard = exclusive();
    let mut report = Report::new("结果导出编码 editor::export（1 万行 × 10 列，纯编码）");

    let entry = export_entry(ROWS, COLS);
    assert_eq!(entry.rows.len(), ROWS);
    assert_eq!(entry.columns.len(), COLS);
    assert!(entry.has_grid());

    // 测的是真东西：三档都真编出了内容，且形状对得上（不是空串早退 / 少拼了一半）
    let csv = export::encode(&entry, ExportFormat::Csv, "public.orders");
    let json = export::encode(&entry, ExportFormat::Json, "public.orders");
    let insert = export::encode(&entry, ExportFormat::Insert, "public.orders");
    assert_eq!(
        csv.lines().next(),
        Some("id,user_id,status,total,currency,created_at,note,city,deleted_at,extra"),
        "CSV 表头"
    );
    assert_eq!(csv.lines().count(), ROWS + 1, "CSV：表头 + 每行一条");
    // CSV 的转义形状：整段用双引号包住 + 内部引号双写（RFC 4180）
    assert!(
        csv.contains(r#""备注,含逗号与""引号"" 0""#),
        "CSV 要按 RFC 4180 转义"
    );
    assert!(json.starts_with('[') && json.ends_with(']'), "JSON 是数组");
    assert!(json.contains("\"deleted_at\": null"), "JSON 里 NULL → null");
    assert_eq!(
        insert.matches("INSERT INTO").count(),
        INSERT_STATEMENTS,
        "INSERT 每 200 行一条语句"
    );
    assert!(insert.contains("'北京'"), "INSERT 的字符串值要带引号");
    assert!(
        insert.starts_with("INSERT INTO \"public.orders\" (id, user_id,"),
        "INSERT 目标名带点号 → 按标识符引号写；列名是简单名 → 不加引号"
    );

    report.note(format!(
        "输入：{} 行 × {} 列；产出 CSV {} / JSON {} / INSERT {}",
        ROWS,
        COLS,
        kb(&csv),
        kb(&json),
        kb(&insert)
    ));

    report.record(
        measure("导出 CSV · 1 万行 × 10 列", 2, 20, || {
            export::encode(&entry, ExportFormat::Csv, "public.orders").len()
        })
        .with_unit("每行", ROWS as f64),
        1_000.0,
        "实测 p95 181~233ms（约 12~17µs/行）；1000ms ≈ 4.3 倍余量",
    );
    report.record(
        measure("导出 JSON · 1 万行 × 10 列", 2, 20, || {
            export::encode(&entry, ExportFormat::Json, "public.orders").len()
        })
        .with_unit("每行", ROWS as f64),
        1_400.0,
        "实测 p95 201~313ms（约 15~20µs/行）；1400ms ≈ 4.5 倍余量",
    );
    report.record(
        measure("导出 INSERT · 1 万行 × 10 列", 2, 20, || {
            export::encode(&entry, ExportFormat::Insert, "public.orders").len()
        })
        .with_unit("每行", ROWS as f64),
        1_000.0,
        "实测 p95 129~225ms（约 10~17µs/行）；1000ms ≈ 4.4 倍余量",
    );

    report.finish();
}
