// ==================== 预览/取样 ====================

use super::*;

impl MockPanel {
    /// 重新加载「可导入结构的连接」与「分析库既有表」（打开面板 / 切换项目 / 下拉刷新）。
    pub fn refresh_sources(&mut self, cx: &mut Context<Self>) {
        self.sources = self.host.schema_sources();
        self.existing_tables = self.host.existing_tables();
        cx.notify();
    }

    /// 只重读「导入结构」的连接候选（打开导入对话框前 / 连接清册变化时）。
    /// 与 [`MockPanel::refresh_sources`] 分开的原因：连接候选从内存里的连接清册派生，
    /// 一次遍历就够，可以跟着连接的新建 / 编辑随时刷；而「既有分析表」那一份要开
    /// 项目分析库文件（真 I/O），只留给面板打开与切项目那一拍。
    pub fn refresh_connection_sources(&mut self, cx: &mut Context<Self>) {
        self.sources = self.host.schema_sources();
        cx.notify();
    }

    /// 字段区的列搜索框（懒创建；`None` = 这张 tab 还没渲染过）。
    ///
    /// `pub` 而不是 `pub(crate)`：面板的只读访问器一律公开（宿主 / 详情 tab / 测试都从这儿读，
    /// 与 `scenario_templates()` / `job_kind()` 同口径），不为了少一个 `pub` 让测试去摸私有字段。
    pub fn column_filter_input(&self) -> Option<Entity<InputState>> {
        self.column_filters.get(&self.config_id).cloned()
    }

    /// 某份配置的列搜索词（空 = 不筛）。搜索词就存在输入框实体里，不另存一份。
    pub(crate) fn column_filter_query(&self, id: u64, cx: &App) -> String {
        self.column_filters
            .get(&id)
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default()
    }

    /// 字段区表头行里的列搜索框（靠右，固定宽）。
    ///
    /// 搜索框空着也不隐藏：一个偶尔冒出来的控件比一个一直在那儿的小输入框更让人意外。
    /// 无列时传 `enabled = false`：**留着位置只降不透明度**，不加第一列时工具栏不会凭空多出一个框。
    pub(crate) fn render_column_filter(
        &mut self,
        id: u64,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.ensure_inputs_for(id, window, cx);
        let theme = cx.theme().clone();
        match self.column_filters.get(&id).cloned() {
            // 边框自己画：组件默认的 `input` 描边在暗色下几乎看不见，直接看不出这里能输入。
            // 换成 `background` 内凹底 + `border` 描边 + 圆角，与右侧 xsmall 按钮同高（24px）。
            Some(input) => div()
                .flex_none()
                .w(rems(ui::COLUMN_FILTER_WIDTH))
                .h_6()
                .h_flex()
                .items_center()
                .px_2()
                .rounded(theme.radius)
                .bg(theme.colors.background)
                .border_1()
                .border_color(theme.colors.border)
                .opacity(if enabled { 1.0 } else { 0.45 })
                .child(
                    Input::new(&input)
                        .appearance(false)
                        .bordered(false)
                        .xsmall(),
                ),
            None => div().flex_none().w(rems(ui::COLUMN_FILTER_WIDTH)),
        }
    }

    // ==================== 预览的按列重排（表头 / 右键菜单） ====================

    /// 这一表当前该显示的取样，与生效中的排序（`None` = 按生成顺序）。
    ///
    /// 缓存只对**派生它的那份取样**与**当时的行数档**有效：结果被换掉（重新生成 / 切项目）、
    /// 或行数档改了就自动失效（见 [`PreviewCache`]），不需要谁记得来清。
    pub fn preview_for(&self, table: &str) -> Option<(&MockPreview, Option<&PreviewSort>)> {
        let info = self.results.iter().find(|info| info.table_name == table)?;
        let cache = self
            .preview_cache
            .as_ref()
            .filter(|cache| cache.applies_to(table, &info.preview, self.preview_limit));
        Some((
            cache.map_or(&info.preview, |cache| &cache.preview),
            cache.and_then(|cache| cache.sort.as_ref()),
        ))
    }

    /// 预览要显示多少行（面板上的「行数」档）。
    pub fn preview_limit(&self) -> usize {
        self.preview_limit
    }

    /// 改预览行数档（面板菜单）：超出生成的取样（[`PREVIEW_ROWS`]）就要重查。
    ///
    /// 与 `sort_preview` 同一处缓存、同一套失效规则：有排序就带着排序重查，没有就只多要几行；
    /// 档位回到 [`PREVIEW_ROWS`] 且没有排序时直接丢掉缓存（生成那份取样就够，不必查库）。
    pub fn set_preview_limit(&mut self, limit: usize, cx: &mut Context<Self>) {
        if limit == self.preview_limit || limit == 0 {
            return;
        }
        let previous = std::mem::replace(&mut self.preview_limit, limit);
        // 带着当前排序重查（缓存里有排序就沿用；没有就是「多要几行」）
        let order = self
            .preview_cache
            .as_ref()
            .filter(|cache| cache.limit == previous)
            .and_then(|cache| cache.sort.clone());
        self.refresh_preview_sample(order, cx);
    }

    /// 表头箭头 / 右键菜单：把预览取样换成**按该列排序后的前 N 行**；
    /// `descending = None` = 取消排序（回到生成顺序；行数档大于生成取样时仍然重查）。
    ///
    /// 只有面板能走这三条出口：重查是 I/O（要走宿主端口）、而重查要的临时表名在结果清单里。
    /// 失败与「内存库正忙」都只写一句可读原因，**不动**已有的取样——预览是只读观察窗，
    /// 点一下箭头不应该把已经在看的数据弄没。
    pub fn sort_preview(
        &mut self,
        table: &str,
        column: &str,
        descending: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let order = descending.map(|descending| PreviewSort {
            column: column.to_string(),
            descending,
        });
        self.refresh_preview_sample_for(table, order, cx);
    }

    /// 重查取样（当前表 = 「当前结果表」，排序可空）。行数档回到生成取样且无排序时退化成「丢缓存」。
    pub(crate) fn refresh_preview_sample(&mut self, order: Option<PreviewSort>, cx: &mut Context<Self>) {
        let table = self.current_info().map(|info| info.table_name.clone());
        let Some(table) = table else {
            return;
        };
        self.refresh_preview_sample_for(&table, order, cx);
    }

    /// 重查取样（指定表；排序可空）。
    ///
    /// 无事可做的情形（只清缓存、不查库）：目标行数 ≤ 生成取样且有排序要取消 / 未变。
    pub(crate) fn refresh_preview_sample_for(
        &mut self,
        table: &str,
        order: Option<PreviewSort>,
        cx: &mut Context<Self>,
    ) {
        let limit = self.preview_limit;
        let Some(info) = self.results.iter().find(|info| info.table_name == table) else {
            return;
        };
        // 不需要查库：不排序且只要生成那几行——直接丢缓存回落到生成取样
        if order.is_none() && limit <= PREVIEW_ROWS {
            let changed = self.preview_cache.is_some();
            self.preview_cache = None;
            if changed {
                cx.notify();
            }
            return;
        }
        // 借租约不能跨到宿主调用（`preview_sample` 是同步 I/O）：先把要用的取出来
        let temp_table = info.temp_table_name.clone();
        let base = info.preview.clone();
        let hint = order
            .as_ref()
            .map(|order| order.label())
            .unwrap_or_else(|| "取样".to_string());
        let queried = self.host.preview_sample(
            &temp_table,
            order.as_ref().map(|o| (o.column.as_str(), o.descending)),
            limit,
        );
        match queried {
            Ok(Some(preview)) => {
                self.preview_cache = Some(PreviewCache {
                    table: table.to_string(),
                    base,
                    limit,
                    sort: order,
                    preview,
                });
                // 成功就把上一次的失败说明收掉（同一拍里换，不留陈旧错误）
                self.error = None;
            }
            // 内存库被别的任务占着：原因说清楚，取样保持原样
            Ok(None) => {
                self.error = Some(format!(
                    "内存库正被别的任务占用（生成 / 出口进行中）：{hint}稍后再试"
                ));
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }
}
