//! 洞察模型 · 历史与版本对比（自 model.rs 拆分，纯位移）

use super::*;
/// 历史列表一页条数。
///
/// 查询上限与界面提示共用这一处：两处各写一个数字，提示迟早与事实不符。
/// 上限本身是**可摘信息**（更老的版本还在库里，只是没列出来），所以界面必须明示
/// （与采样提示 D15 同一立场：别让人把截断的列表当成全部）。
pub const HISTORY_PAGE_SIZE: usize = 10;

/// 清理旧快照时默认保留的天数。
///
/// **界面文案与实际切档共用这一处**（对话框写「N 天前」、接缝按同一个 N 删），
/// 否则会出现「说好清 30 天却按 7 天删」这类最坏的不一致。
///
/// 取值待拍板（架构 §12 **Q4**）：先按原型的 30 天落地——它只影响「多久算旧」，
/// 与「每列最多留 N 版」（`store::body::MAX_VERSIONS_PER_COLUMN`）是两道独立的闸。
pub const SNAPSHOT_RETENTION_DAYS: i64 = 30;

/// 快照历史（「历史」Tab，Phase 5.1 / 5.2）
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryView {
    /// 实体名（当前只做列：就是列名）
    pub entity: String,
    /// 版本列表（最新在前）
    pub entries: Vec<HistoryEntryView>,
    /// 存储用量（取不到就不显示数字，而不是显示 0）
    pub stats: Option<StorageStatsView>,
    /// 列表是否被分页截断（列表已满一页；不代表库里一定还有更多）
    pub truncated: bool,
    /// 版本对比（Phase 5.2）：`None` = 没在对比（面板不出现）。
    ///
    /// 对比结果也在**载荷**里而不在视图字段里：面板「有没有对比"只认载荷，
    /// 视图只管「用户点了哪一版」（与 D35「渲染以载荷为准」同一口径）。
    pub diff: Option<VersionDiffView>,
    /// 上次清理的回执（Phase 5.3）：同样是载荷的一部分——下次刷新自然消失，
    /// 正是「一次性动作结果」该有的寿命。
    pub cleanup: Option<CleanupOutcome>,
}

/// 一次清理的结果（面板给一行回执）。
///
/// 两侧条数分开报：正文与元数据是**成对写入**的（D16），删的时候也必须成对——
/// 两边对不上就是半写的信号，不能被一句「清理成功」盖过去。
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupOutcome {
    /// 切档用的天数（界面写的就是这个数）
    pub days: i64,
    /// 正文删掉的条数（项目 DuckDB）
    pub body_removed: usize,
    /// 元数据删掉的条数（项目 SQLite）
    pub meta_removed: usize,
}

impl CleanupOutcome {
    /// 两侧对上了（没有半写残留）
    pub fn is_balanced(&self) -> bool {
        self.body_removed == self.meta_removed
    }

    /// 面板那行回执
    pub fn summary(&self) -> String {
        if self.body_removed == 0 && self.meta_removed == 0 {
            return format!("没有 {} 天前的快照", self.days);
        }
        if !self.is_balanced() {
            return format!(
                "正文删了 {} 条、版本链删了 {} 条，两侧对不上（可能有一次半写）",
                self.body_removed, self.meta_removed
            );
        }
        format!("清理了 {} 条快照（{} 天前）", self.body_removed, self.days)
    }
}

/// 一个版本（列表里的一行）
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntryView {
    pub version_id: String,
    /// 短版本号（前 8 位；完整值太长，列表里放不下）
    pub short_version: String,
    pub created_at: String,
    pub data_type: String,
    /// 父版本（首版为 `None`）：版本链靠它串起来
    pub has_parent: bool,
    /// 【K14】有父版本、但父版**不在**这份列表里——链的起段被清理剪掉了。
    ///
    /// 只在列表**没被分页截断**时才判：截断时最后一页的最旧一版本来就看不到父版，
    /// 那不是说「被清理了」。
    pub chain_truncated: bool,
    /// 是否最新一版（列表里打「当前」标记）
    pub is_latest: bool,
}

/// 存储用量（面板底部的统计行）
#[derive(Debug, Clone, PartialEq)]
pub struct StorageStatsView {
    pub total_snapshots: usize,
    pub unique_columns: usize,
    /// 后端给的展示串（带单位），面板不自己换算
    pub size_display: String,
}

/// 版本对比（`HistoryView::diff`，Phase 5.2）。
///
/// 方向固定为**「选中的那一版 → 最新一版」**：这个 Tab 问的是「和上次比变了什么」，
/// 两个方向都能选只是多一个状态（还要多一套「谁是基准」的文案）。
#[derive(Debug, Clone, PartialEq)]
pub struct VersionDiffView {
    /// 基准版本（对比面板标题里的那个时间）
    pub baseline_version: String,
    pub baseline_label: String,
    pub latest_label: String,
    pub rows: Vec<DiffRowView>,
    /// 有变化的行数（面板头据此给结论，不数第二遍）
    pub changed: usize,
}

/// 对比面板的一行：`旧 → 新` + 差值
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRowView {
    pub label: String,
    /// 基准版本的展示值（新类型不再产这行时是「—」，不假装还有值）
    pub old: String,
    pub new: String,
    pub delta: DeltaView,
}

/// 变化方向。
///
/// **方向 ≠ 好坏**：空值率升是坏，唯一值升不一定是好，所以这里只给方向，
/// 由视图映射到中性的主题角色（不用 `success` / `danger` 预设评价）。
#[derive(Debug, Clone, PartialEq)]
pub enum DeltaView {
    /// 数值增加（文案已带符号与单位）
    Up(String),
    /// 数值减少
    Down(String),
    /// 两侧展示值完全一致
    Same,
    /// 变了，但算不出数值差（类型 / 区间 / 带单位的文本值）
    Changed,
}

impl DeltaView {
    /// 面板头数「几项有变化」用
    pub fn is_changed(&self) -> bool {
        !matches!(self, DeltaView::Same)
    }

    /// 差值文案（不变 / 已变这两档没有数字）
    pub fn text(&self) -> &str {
        match self {
            DeltaView::Up(text) | DeltaView::Down(text) => text,
            DeltaView::Same => "不变",
            DeltaView::Changed => "已变",
        }
    }
}

impl VersionDiffView {
    /// 两份领域画像 → 对比视图（纯函数）。
    ///
    /// 行集合与「列」Tab 的基础统计**同源**（同一个 [`ColumnProfileView`]）：
    /// 对比里出现的数字就是用户已经读过的那几个，不另立一套口径。
    pub fn between(
        baseline: &ColumnInsightFull,
        baseline_label: &str,
        latest: &ColumnInsightFull,
        latest_label: &str,
    ) -> Self {
        let old = ColumnProfileView::from_domain(baseline);
        let new = ColumnProfileView::from_domain(latest);
        let s = &baseline.stats;
        let n = &latest.stats;

        let mut rows = Vec::new();

        rows.push(DiffRowView::count(
            "总行数",
            s.total_count as f64,
            n.total_count as f64,
        ));
        rows.push(DiffRowView::count(
            "非空值",
            s.total_count.saturating_sub(s.null_count) as f64,
            n.total_count.saturating_sub(n.null_count) as f64,
        ));
        // 「空值」与「空值率」拆成两行：「列」Tab 把它俩写成了一行
        // （`12（14.3%）`），而对比要的是能对齐的两个数
        rows.push(DiffRowView::count(
            "空值",
            s.null_count as f64,
            n.null_count as f64,
        ));
        rows.push(DiffRowView::rate("空值率", s.null_rate, n.null_rate));
        rows.push(match (s.unique_count, n.unique_count) {
            (Some(a), Some(b)) => DiffRowView::count("唯一值", a as f64, b as f64),
            // 至少一侧算不出唯一值（未识别类型）就不给差值
            _ => DiffRowView::text("唯一值", "—", "—"),
        });
        rows.push(DiffRowView::text("类型", &s.data_type, &n.data_type));

        // 类型专属行（平均 / 长度范围 / 跨度 / True 占比 …）逐行对齐：
        // 能解成数字就给数值差，否则只说「已变」（区间 / 类别串算不出差值）
        for row in &new.basics {
            if CANONICAL_LABELS.contains(&row.label) {
                continue;
            }
            let paired = old.basics.iter().find(|r| r.label == row.label);
            rows.push(DiffRowView::from_display(
                row.label,
                paired.map(|r| r.value.as_str()),
                &row.value,
            ));
        }
        // 旧版有、新版没有的行（类型变了）：如实列出来，新版侧写「—」
        for row in &old.basics {
            if CANONICAL_LABELS.contains(&row.label)
                || new.basics.iter().any(|r| r.label == row.label)
            {
                continue;
            }
            rows.push(DiffRowView::from_display(row.label, Some(&row.value), "—"));
        }

        let changed = rows.iter().filter(|r| r.delta.is_changed()).count();
        Self {
            baseline_version: String::new(),
            baseline_label: baseline_label.to_string(),
            latest_label: latest_label.to_string(),
            rows,
            changed,
        }
    }

    /// 挂上基准版本号（服务知道对的是哪一版；`between` 只算差值）
    pub fn with_baseline_version(mut self, version_id: &str) -> Self {
        self.baseline_version = version_id.to_string();
        self
    }

    /// 面板头的一句结论
    pub fn summary(&self) -> String {
        if self.changed == 0 {
            return format!("与 {latest} 完全一致", latest = self.latest_label);
        }
        format!("{} 项有变化", self.changed)
    }
}

/// 对比里我亲自给出的行（不参与「类型专属行」的二次扫描）
const CANONICAL_LABELS: [&str; 5] = ["总行数", "非空值", "空值", "唯一值", "类型"];

impl DiffRowView {
    /// 计数行（整数展示 + 整数差）
    fn count(label: &str, old: f64, new: f64) -> Self {
        Self {
            label: label.to_string(),
            old: fmt_int(old.max(0.0) as u32),
            new: fmt_int(new.max(0.0) as u32),
            delta: delta_of(old, new, |v| fmt_int(v as u32), ""),
        }
    }

    /// 占比行（1 位小数的百分数）
    fn rate(label: &str, old: f64, new: f64) -> Self {
        // 先按**展示精度**取整再算差：否则会出现「显示 8.2% → 8.2% 却 +0.01」的自相矛盾
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        let (a, b) = (round(old), round(new));
        Self {
            label: label.to_string(),
            old: fmt_pct(a),
            new: fmt_pct(b),
            delta: delta_of(a, b, fmt_pct, ""),
        }
    }

    /// 纯文本行（类型 / 唯一值这类两侧都是字符串的）
    fn text(label: &str, old: &str, new: &str) -> Self {
        Self::from_display(label, Some(old), new)
    }

    /// 文本行：能解成数字就给数值差，否则只说变没变
    fn from_display(label: &str, old: Option<&str>, new: &str) -> Self {
        let old_text = old.unwrap_or("—").to_string();
        let delta = match (old.and_then(display_number), display_number(new)) {
            (Some((a, unit_a)), Some((b, unit_b))) if unit_a == unit_b => {
                let suffix = if unit_a { "%" } else { "" };
                delta_of(a, b, fmt_delta_number, suffix)
            }
            _ if old_text == new => DeltaView::Same,
            _ => DeltaView::Changed,
        };
        Self {
            label: label.to_string(),
            old: old_text,
            new: new.to_string(),
            delta,
        }
    }
}

/// 方向 + 差值文案（差为零就是「不变」；`show` 拿到的是**绝对值**，符号由这里加）
fn delta_of(old: f64, new: f64, show: impl Fn(f64) -> String, suffix: &str) -> DeltaView {
    let diff = new - old;
    if diff.abs() < f64::EPSILON {
        return DeltaView::Same;
    }
    let sign = if diff > 0.0 { "+" } else { "-" };
    let text = format!("{sign}{}{suffix}", show(diff.abs()));
    if diff > 0.0 {
        DeltaView::Up(text)
    } else {
        DeltaView::Down(text)
    }
}

/// 差值里的数值文案：整数加千分位（与计数行同一口径），小数去尾零
fn fmt_delta_number(v: f64) -> String {
    if v.fract() == 0.0 && v < u32::MAX as f64 {
        return fmt_int(v as u32);
    }
    fmt_num(v)
}

/// 展示串 → 数值（只认「纯数值」：数字 + 千分位 + 可有 `%` 后缀）。
///
/// 带单位 / 区间 / 类别串（`1.2 万` / `3–8` / `Top 3 类` / `2.5（右偏）`）一律不认——
/// 宁可不给差值，也不给一个错的差值。返回值第二项表示「这是百分数」。
fn display_number(text: &str) -> Option<(f64, bool)> {
    let trimmed = text.trim();
    let (body, percent) = match trimmed.strip_suffix('%') {
        Some(rest) => (rest, true),
        None => (trimmed, false),
    };
    let parsed = body.replace(',', "").parse::<f64>().ok()?;
    parsed.is_finite().then_some((parsed, percent))
}

impl HistoryView {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 领域版本列表 + 存储统计 → 视图模型（纯函数）。
    ///
    /// `entries` 由存储层按「最新在前」返回（`ORDER BY created_at DESC, rowid DESC`），
    /// 这里不再排序：**显示顺序与写入顺序只能有一处权威**。
    pub fn from_entries(
        entity: &str,
        entries: &[crate::store::InsightVersionEntry],
        stats: Option<&crate::store::InsightStorageStats>,
    ) -> Self {
        Self {
            entity: entity.to_string(),
            entries: entries
                .iter()
                .enumerate()
                .map(|(index, entry)| HistoryEntryView {
                    version_id: entry.version_id.clone(),
                    short_version: entry.version_id.chars().take(8).collect(),
                    created_at: entry.created_at.clone(),
                    data_type: entry.data_type.clone().unwrap_or_else(|| "—".to_string()),
                    has_parent: entry.parent_version_id.is_some(),
                    chain_truncated: {
                        // 三条件同时成立才能说链被剪过：「列表完整（未分页截断）」+
                        // 「最旧一版」+「父版不在此列」——否则就是分页造成的假阳性（K14）
                        let is_oldest = index + 1 == entries.len();
                        let page_complete = entries.len() < HISTORY_PAGE_SIZE;
                        is_oldest
                            && page_complete
                            && entry.parent_version_id.as_deref().is_some_and(|parent| {
                                !entries.iter().any(|other| other.version_id == parent)
                            })
                    },
                    is_latest: index == 0,
                })
                .collect(),
            truncated: entries.len() >= HISTORY_PAGE_SIZE,
            diff: None,
            cleanup: None,
            stats: stats.map(|stats| StorageStatsView {
                total_snapshots: stats.total_snapshots,
                unique_columns: stats.unique_columns,
                size_display: stats.total_size_display.clone(),
            }),
        }
    }

    /// 存储统计行文案（拿不到统计返回 `None`，不编一个 0 出来）
    pub fn stats_line(&self) -> Option<String> {
        let stats = self.stats.as_ref()?;
        Some(format!(
            "存储 {} · {} 个快照 · {} 列",
            stats.size_display, stats.total_snapshots, stats.unique_columns
        ))
    }

    /// 挂上对比结果（服务读两版正文后调用）
    pub fn with_diff(mut self, diff: VersionDiffView) -> Self {
        self.diff = Some(diff);
        self
    }

    /// 放下对比结果（用户点 ✕，或基准版已不在列表里）
    pub fn without_diff(mut self) -> Self {
        self.diff = None;
        self
    }

    /// 挂上清理回执（服务删完再读回列表时调用）
    pub fn with_cleanup(mut self, outcome: CleanupOutcome) -> Self {
        self.cleanup = Some(outcome);
        self
    }
}