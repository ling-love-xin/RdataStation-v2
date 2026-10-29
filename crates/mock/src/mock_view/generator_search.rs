// ==================== 生成器搜索（第二条路径） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

/// 搜索生成器目录：按**中文标签 / 名称 / 分类名**匹配（大小写不敏感，多词之间是 AND）。
///
/// 排序（越靠前越像用户要找的）：标签前缀 → 名称前缀 → 标签包含 → 名称包含 → 分类名包含；
/// 同级保持目录顺序。空查询返回全部 143 项（对话框初态）。
///
/// 搜索是“知道大概叫什么”的路径；按分类翻菜单是“知道属于哪类”的路径，两者并存（D24）。
pub fn search_generators(query: &str) -> Vec<&'static GeneratorSpec> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(u8, usize, &'static GeneratorSpec)> = Vec::new();
    for (index, spec) in generator_catalog::all_specs().iter().copied().enumerate() {
        let label = spec.label.to_lowercase();
        let name = spec.name.to_lowercase();
        let category = spec.category.label().to_lowercase();
        let mut rank = 0u8;
        let mut matched = true;
        for term in terms.iter() {
            // 命中强弱：标签前缀 0 < 名称前缀 1 < 标签包含 2 < 名称包含 3 < 分类名 4
            let hit = if label.starts_with(term.as_str()) {
                0
            } else if name.starts_with(term.as_str()) {
                1
            } else if label.contains(term.as_str()) {
                2
            } else if name.contains(term.as_str()) {
                3
            } else if category.contains(term.as_str()) {
                4
            } else {
                matched = false;
                break;
            };
            rank = rank.max(hit);
        }
        if matched {
            hits.push((rank, index, spec));
        }
    }
    hits.sort_by_key(|(rank, index, _)| (*rank, *index));
    hits.into_iter().map(|(_, _, spec)| spec).collect()
}

/// 「搜索生成器」列表的委托：内层靠 `List` 组件（自带搜索框 / 虚拟化 / 回车与点击确认）。
///
/// 目录只有 143 项且全在内存：`perform_search` 同步过滤，不走异步搜索通道（无需 loading 占位）。
pub(crate) struct GeneratorSearchDelegate {
    /// 选中后写回的面板（草稿的单一权威）
    panel: Entity<MockPanel>,
    /// 目标列 id
    column_id: u64,
    /// 该列当前在用的生成器（列表里打勾；`None` = 列已被删）
    current: Option<&'static str>,
    /// 命中项（`search_generators` 的结果）
    hits: Vec<&'static GeneratorSpec>,
    /// 列表选中项（上下键 / 鼠标悬停）
    selected: Option<IndexPath>,
}

impl GeneratorSearchDelegate {
    /// 建委托：初态＝全量目录（143 项）+ 勾出该列当前的生成器。
    ///
    /// `current` 由调用方（面板自己的 `&mut self`）算好传入：本方法在面板的 update 里被调用，
    /// 那时 `read` 面板会触发「already being updated」重入 panic。
    pub(crate) fn new(panel: Entity<MockPanel>, column_id: u64, current: Option<&'static str>) -> Self {
        Self {
            panel,
            column_id,
            current,
            hits: search_generators(""),
            selected: None,
        }
    }

    /// 命中项（仅测试观察：生产侧不需要读它）
    #[cfg(test)]
    pub(crate) fn hits(&self) -> &[&'static GeneratorSpec] {
        &self.hits
    }
}

impl ListDelegate for GeneratorSearchDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.hits = search_generators(query);
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.hits.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let spec = *self.hits.get(ix.row)?;
        let muted = cx.theme().colors.muted_foreground;
        // 一行三列：中文标签（主）/ 名称 / 分类（辅）；当前在用的那项打勾
        Some(
            ListItem::new(ix.row)
                .confirmed(self.current == Some(spec.name))
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .w_full()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_ellipsis()
                                .child(spec.label),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(spec.name),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(spec.category.label()),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }

    /// 点一行 / 回车：写回该列的生成器并关掉对话框。
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(spec) = self.selected.and_then(|ix| self.hits.get(ix.row).copied()) else {
            return;
        };
        let name = spec.name;
        self.panel.update(cx, |panel, cx| {
            panel.set_generator(self.column_id, name, cx)
        });
        window.close_dialog(cx);
    }
}

/// 字段区列搜索的匹配规则：**每个词**都要在列名 / 生成器名 / 类型名 / 置信度里至少命中一个
/// （子串、大小写不敏感）；空查询留下全部。
///
/// 四个靶子的理由：
/// - 列名：最常见的用法；
/// - 生成器名：人常常记得的是「那列是邮箱」，而不是列名（生成器标签是中文）；
/// - 类型名：想找「所有时间列」「所有整数列」时看的就是卡片上那个类型（`column_type_label`
///   给的是 DuckDB 类型名：`INTEGER` / `VARCHAR` / `TIMESTAMP` …）；
/// - 置信度：卡片上就写着 `high` / `low` / `manual`——要复核「按类型猜的」那些列时搜 `low`。
///
/// 多词是 **AND**（与「搜索生成器」对话框同一约定）：`id INTEGER` 只留两条都中的列。
pub fn column_matches(
    query: &str,
    name: &str,
    generator_label: &str,
    type_label: &str,
    confidence: &str,
) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let haystacks = [
        name.to_lowercase(),
        generator_label.to_lowercase(),
        type_label.to_lowercase(),
        confidence.to_lowercase(),
    ];
    query.split_whitespace().all(|term| {
        let term = term.to_lowercase();
        haystacks.iter().any(|text| text.contains(&term))
    })
}

