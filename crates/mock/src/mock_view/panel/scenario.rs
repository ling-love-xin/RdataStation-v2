// ==================== 场景/关系 ====================

use super::*;

impl MockPanel {
    /// 再开始生成（关系是这次生成的一部分，不能生成完再补——那就要去改已落地的数据了）。
    pub fn open_scenario(&mut self, template_id: &str, cx: &mut Context<Self>) {
        if self.is_running() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        let Some(template) = crate::templates::get_template_by_id(template_id) else {
            self.fail(format!("场景模板不存在：{template_id}"), cx);
            return;
        };
        self.load_scenario(template, cx);
    }

    /// 把**当前表配置**加进场景工作副本（自定义多表场景的入口）。
    ///
    /// 为何从表配置来：导入源库结构、智能映射、列编辑、行数 / 种子这一整套都在单表态里
    /// 现成可用——“把一张表调到满意，然后把它加进场景”比再做一个多表编辑器便宜得多，
    /// 也不破坏现有交互。
    pub fn add_draft_to_scenario(&mut self, cx: &mut Context<Self>) {
        let name = self.draft.table_name.trim().to_string();
        if name.is_empty() {
            self.fail("表配置的目标表名为空：先在「目标表名」里填一个", cx);
            return;
        }
        if self.draft.columns.is_empty() {
            self.fail("表配置还没有列：先导入源库结构或手工加列", cx);
            return;
        }
        let Some(template) = self.scenario.as_mut() else {
            self.fail("先选一套场景模板（或在模板基础上改）", cx);
            return;
        };
        if template.tables.iter().any(|table| table.name == name) {
            self.fail(
                format!("场景里已经有表 {name}：先删掉那张，或改表配置的目标表名"),
                cx,
            );
            return;
        }
        let table = TemplateTable {
            name: name.clone(),
            row_count: self.draft.options.rows,
            columns: self.draft.columns.iter().map(|c| c.def.clone()).collect(),
        };
        template.tables.push(table);
        self.error = None;
        self.outcome = Some(format!(
            "已把表配置 {name}（{} 行 · {} 列）加进本次生成",
            with_thousands(u64::from(self.draft.options.rows)),
            self.draft.columns.len()
        ));
        cx.notify();
    }

    /// 从场景工作副本里删掉一张表，**并连带清掉指向它的关系**。
    ///
    /// 不清的话，剩余引用会变成“指向模板里没有的表”——生成前校验会报错，
    /// 但那时用户已经不知道是谁指向它了。所以这里直接清掉并报出清了几条。
    pub fn remove_scenario_table(&mut self, table: &str, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.as_mut() else {
            return;
        };
        let before = template.tables.len();
        template.tables.retain(|t| t.name != table);
        if template.tables.len() == before {
            return;
        }
        let mut dropped = 0usize;
        for child in template.tables.iter_mut() {
            for column in child.columns.iter_mut() {
                let points_here = column
                    .dependency
                    .as_ref()
                    .is_some_and(|dep| dep.ref_table == table);
                if points_here {
                    column.dependency = None;
                    dropped += 1;
                }
            }
        }
        self.error = None;
        self.outcome = Some(if dropped == 0 {
            format!("已从本次生成里删掉表 {table}")
        } else {
            format!("已删掉表 {table}，并清掉 {dropped} 条指向它的关系")
        });
        cx.notify();
    }

    /// 载入场景工作副本（[`Self::open_scenario`] 按 id 取到模板后调它；测试直接给模板）。
    pub(crate) fn load_scenario(&mut self, template: ScenarioTemplate, cx: &mut Context<Self>) {
        self.scenario = Some(template.clone());
        // S5：种子输入跟着模板走（重放 / 换模板时把当时的 seed 摆回来）
        self.scenario_seed_pending = template.seed.map(|seed| seed.to_string());
        *self.relation_pick.borrow_mut() = RelationPick::default();
        // 已结果及其关系快照不动：载入工作副本不等于重新生成，旧结果仍然能导出
        self.error = None;
        self.outcome = None;
        cx.notify();
    }

    /// 退出场景态（丢弃工作副本，表配置与已有结果不受影响）。
    pub fn close_scenario(&mut self, cx: &mut Context<Self>) {
        self.scenario = None;
        *self.relation_pick.borrow_mut() = RelationPick::default();
        cx.notify();
    }

    /// 工具栏「▦ 新增场景」：从空白场景（0 张表）开始建工作副本。
    ///
    /// 与「选模板」同一入口（[`Self::load_scenario`]）：进来的是 0 张表的模板，
    /// 之后用「＋ 加表（当前表配置）」「＋ 加关系」从零搭起（`add_draft_to_scenario` 的
    /// 「先选一套场景模板」门槛由空模板本身满足）。
    pub fn new_blank_scene(&mut self, cx: &mut Context<Self>) {
        if self.is_running() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        self.load_scenario(crate::templates::blank_scene_template(), cx);
    }

    /// 工具栏「＋」：新建一张表配置（对齐原型 `newMockTable`——每次点击从一张新表开始配）。
    ///
    /// 单表态的表配置只有一份（清单那一行就是它，design.md 单表态 = 表配置与结果一行），
    /// 所以「清单加一条记录」落到实现里 = 把表配置换成一张**新表**（表名递增、列清空、
    /// 行数复位），并聚焦它的中央 tab（配字段 / 名称都在 tab 里）；上一张若已生成，
    /// 结果仍留在清单（`results`），新表生成后再续一条。
    pub fn new_table_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_seq += 1;
        let id = u64::from(self.draft_seq);
        let locale = self.draft.options.locale.clone();
        // 上一份收进存储：它自己的 tab 还开着，切回去内容不能丢——这就是「＋」不再
        // 覆盖当前 tab 的全部实现（以前只有一份 `draft`，新表把旧表就地替换掉）。
        let previous = std::mem::replace(
            &mut self.draft,
            MockTableConfig {
                table_name: format!("mock_table_{id}"),
                columns: Vec::new(),
                options: MockRunOptions::new(1000, None, locale),
            },
        );
        self.configs.insert(self.config_id, previous);
        self.config_id = id;
        // 新表自带一列：空表起手得先读一段「还没有列」的空态说明才知道能干嘛，
        // 自带一列就能直接改参数 / 生成。命名与生成器都沿用「＋ 加列」的口径
        // （`column_1` + 该类型的默认生成器），不另立特例——否则新表首列与后加的
        // 列行为不一致，反而更费解。
        self.add_column(
            ui::DEFAULT_FIRST_COLUMN_NAME.to_string(),
            ColumnDataType::Integer,
            cx,
        );
        // 新表从「未生成」开始：清掉上一张的生成反馈（error 留给失败现场，这里不误清）
        self.outcome = None;
        self.error = None;
        cx.notify();
        self.open_detail_for(DetailTarget::Config(id), window, cx);
    }

    /// 工具栏「⟳ 刷新」：重读内存临时表的**当前行数**，同步清单行尾数字。
    ///
    /// 测试过程中可能改过这张表（加字段 / 增删行 / 外部写入）：`results` 里的行数是
    /// 生成时的快照，刷新把它与库内现状对齐。查不到（连接正忙 / 表已被清）保持现状，
    /// 不打断用户——刷新是随手动作，不为它弹错。
    pub fn refresh_tables(&mut self, cx: &mut Context<Self>) {
        let mut changed = 0usize;
        for info in self.results.iter_mut() {
            if let Ok(Some(rows)) = self.host.temp_table_row_count(&info.temp_table_name) {
                if info.row_count != rows as u32 {
                    info.row_count = rows as u32;
                    changed += 1;
                }
            }
        }
        self.outcome = Some(if changed == 0 {
            "已刷新：临时表行数与清单一致".to_string()
        } else {
            format!("已刷新：{changed} 张表行数已同步")
        });
        self.error = None;
        cx.notify();
    }

    /// 场景态下的「生成」（后台任务：一次产出模板里的全部表；**不写库**）。
    pub fn run_scenario(&mut self, cx: &mut Context<Self>) {
        let Some(mut template) = self.scenario.clone() else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        // S5：种子输入 → 模板 seed（留空 = 随机，与单表 seed 输入同一语义）。
        // 同步在这里而不是 `sync_inputs`：场景不读表配置，输入也只在此刻消费。
        if let Some(input) = self.scenario_seed_input.clone() {
            match parse_seed(&input.read(cx).value()) {
                Ok(seed) => template.seed = seed,
                Err(e) => {
                    self.fail(e, cx);
                    return;
                }
            }
        }
        // 空白场景（0 张表）先别提交：生成空结果没有意义，引导用户先加第一张表
        if template.tables.is_empty() {
            self.fail(
                "空白场景还没有表：先用「＋ 加表（当前表配置）」添加第一张",
                cx,
            );
            return;
        }
        // 生成前校验引用（父表 / 父列不合适就不提交；错误在面板上就地显示）
        if let Err(e) = MockEngine::resolve_reference_domains(&template) {
            self.fail(format!("表间关系有问题：{e}"), cx);
            return;
        }
        self.start_job(MockJobKind::Scenario(Box::new(template)), cx);
    }

    /// 场景工作副本（`None` = 单表态）。
    pub fn scenario(&self) -> Option<&ScenarioTemplate> {
        self.scenario.as_ref()
    }

    /// 关系闭包里**还没落库**的表（按结果表顺序；已落库的跳过）。
    ///
    /// 为何要它：关系是跨表的，而出口以「当前表」为单位——用户很容易只落其中
    /// 几张，留下悬空的引用。这里把「还差哪几张」摆出来，让他一次落完。
    /// 空 = 当前表不参与关系，或关系里的表都落了。
    pub fn pending_relation_tables(&self) -> Vec<String> {
        if self.last_relations.is_empty() {
            return Vec::new();
        }
        let Some(current) = self.current_info().map(|info| info.table_name.clone()) else {
            return Vec::new();
        };
        // 从当前表出发沿关系双向走一遍，得到它与关系闭包（父子链）
        let mut closure = vec![current];
        loop {
            let mut added = false;
            for relation in &self.last_relations {
                for (from, to) in [
                    (&relation.child_table, &relation.parent_table),
                    (&relation.parent_table, &relation.child_table),
                ] {
                    if closure.iter().any(|name| name == from)
                        && !closure.iter().any(|name| name == to)
                    {
                        closure.push(to.clone());
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
        self.results
            .iter()
            .map(|info| info.table_name.clone())
            .filter(|name| closure.iter().any(|c| c == name))
            .filter(|name| !self.landed_tables.iter().any(|landed| landed == name))
            .collect()
    }

    /// 本会话已落库的表。
    pub fn landed_tables(&self) -> &[String] {
        &self.landed_tables
    }

    /// 出口：把关系闭包里还没落库的表**依次**落成分析库新表（后台任务）。
    ///
    /// 逐张走「新建」语义：同名已存在就跳过它并报出原因（不覆盖、也不回滚别人）。
    pub fn persist_related(&mut self, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        let pending = self.pending_relation_tables();
        if pending.is_empty() {
            self.fail("关系里没有待落库的表", cx);
            return;
        }
        tracing::info!(
            tables = ?pending,
            "【探针】Mock「落库这些表」(persist_related) 触发"
        );
        let infos: Vec<MockGenInfo> = self
            .results
            .iter()
            .filter(|info| pending.contains(&info.table_name))
            .cloned()
            .collect();
        self.start_job(MockJobKind::PersistAll(infos), cx);
    }

    /// 批量存草稿箱：把当前所有结果表一次写到 `{项目}/scratchpad/mock/`。
    ///
    /// 与「落库这 N 张」对称：落库走分析库新表，草稿箱走 CSV 文件。
    /// 没项目根直接拦（草稿箱只写项目目录；出口区按钮也据此禁用）。
    pub(crate) fn scratchpad_all(&mut self, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            self.fail("没有结果表可导出", cx);
            return;
        }
        if self.host.project_root().is_none() {
            self.fail("未打开项目：草稿箱只写项目目录", cx);
            return;
        }
        let infos = self.results.clone();
        self.start_job(
            MockJobKind::ScratchpadAll {
                infos,
                format: MockExportFormat::Csv,
            },
            cx,
        );
    }

    /// 当前结果表的**跨表后果**（用于出口提示；`None` = 它不参与任何关系）。
    ///
    /// 为何要给这句：**出口只作用于当前表**，而关系是跨表的——只落子表不落父表，
    /// 落地的数据就悬空了（而 mock 不改已落地的数据，也不会去替用户补）。
    pub fn current_relation_note(&self) -> Option<String> {
        let table = self.current_info()?.table_name.clone();
        self.relation_note_for(&table)
    }

    /// 指定那张表的**跨表后果**（结果表 tab 的表头用：那张 tab 看的可能是别的表，见 D35）。
    pub(crate) fn relation_note_for(&self, table: &str) -> Option<String> {
        let current = table.to_string();
        let mut out: Vec<String> = Vec::new();
        let parents: Vec<String> = self
            .last_relations
            .iter()
            .filter(|r| r.child_table == current)
            .map(|r| {
                format!(
                    "{} → {}.{}",
                    r.child_column, r.parent_table, r.parent_column
                )
            })
            .collect();
        if !parents.is_empty() {
            let mut parents = parents;
            parents.sort();
            parents.dedup();
            out.push(format!(
                "引用了 {}：只落这张表，被引用的表不会跟着落库",
                parents.join("、")
            ));
        }
        let children: Vec<String> = self
            .last_relations
            .iter()
            .filter(|r| r.parent_table == current)
            .map(|r| format!("{}.{}", r.child_table, r.child_column))
            .collect();
        if !children.is_empty() {
            let mut children = children;
            children.sort();
            children.dedup();
            out.push(format!(
                "被 {} 引用：只落这张表，引用它的一方会落空",
                children.join("、")
            ));
        }
        (!out.is_empty()).then(|| out.join("；"))
    }

    /// 最近一次**场景生成**用的关系快照（结果还在就保留；单表生成会清掉）。
    pub fn last_relations(&self) -> &[ScenarioRelation] {
        &self.last_relations
    }
    /// 工作副本里的表间关系（派生视图：扫列上的 `dependency`，不另存清单）。
    pub fn scenario_relations(&self) -> Vec<ScenarioRelation> {
        let Some(template) = self.scenario.as_ref() else {
            return Vec::new();
        };
        relations_of(template)
    }

    /// 关系目标的**取值域**文案（`1..1,000`）；父列自增且父表在册时给出。
    ///
    /// 面板在关系行里显示它：改父表行数前后，用户能直接看到采样域变了——
    /// 这正是“域由父表行数算出”的可见回报。
    pub fn relation_range(&self, relation: &ScenarioRelation) -> Option<String> {
        let template = self.scenario.as_ref()?;
        let parent = template
            .tables
            .iter()
            .find(|t| t.name == relation.parent_table)?;
        let column = parent
            .columns
            .iter()
            .find(|c| c.name == relation.parent_column)?;
        let GeneratorConfig::AutoIncrement { start, step } = column.generator else {
            return None;
        };
        let domain = ReferenceDomain {
            table: parent.name.clone(),
            column: column.name.clone(),
            first: i64::from(start),
            step: i64::from(step),
            count: parent.row_count,
        };
        Some(format!(
            "{}..{}",
            with_thousands(domain.first.max(0) as u64),
            with_thousands(domain.last().max(0) as u64)
        ))
    }

    /// 打开「编辑表」对话框（表名 / 行数）。
    ///
    /// 改行数是场景里最常用的一步（比如把财务模板的 10 万行缩下来），而引用它的表
    /// **自动跟着变**——域由父表行数算出，不需要用户去同步任何东西。
    pub fn open_table_dialog(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self
            .scenario
            .as_ref()
            .and_then(|template| template.tables.iter().find(|t| t.name == table))
            .cloned()
        else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        self.table_editing = Some(current.name.clone());
        // 上一次的错误留在面板上会让人以为是这次的输入有问题：打开就清掉
        self.error = None;
        let name_input = self
            .edit_name_input
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        let rows_input = self
            .edit_rows_input
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("行数")))
            .clone();
        name_input.update(cx, |state, cx| {
            state.set_value(current.name.clone(), window, cx)
        });
        rows_input.update(cx, |state, cx| {
            state.set_value(current.row_count.to_string(), window, cx)
        });

        let panel = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            // 校验没过时对话框不关：错误就地显示，改完能直接再点「应用」
            let error = panel.read(cx).error.clone();
            // 说明 / 表单 / 收尾说明用分隔线断开：否则两段说明与两行表单是同一种灰，
            // 读起来是一整块，分不出「哪句是限制、哪行是输入」
            let mut body = div()
                .v_flex()
                .gap_2()
                .child(hint_line(
                    theme,
                    "只改这张表在本次生成里的配置；已生成的预览与已落地的表都不动。",
                ))
                .child(form_line(theme, "表名", &name_input))
                .child(form_line(theme, "行数", &rows_input))
                .child(card_divider(theme))
                .child(hint_line(
                    theme,
                    "引用它的表会自动跟着变：取值域由父表行数算出（改完下次生成生效）",
                ));
            if let Some(error) = error {
                body = body.child(error_line(theme, error));
            }
            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            dialog.title("编辑表").child(body).footer(
                DialogFooter::new()
                    .child(
                        button("mock-table-cancel", "取消", ActionKind::Soft).on_click(
                            move |_, window, app| {
                                let _ = cancel_panel
                                    .update(app, |panel, _cx| panel.table_editing = None);
                                window.close_dialog(app);
                            },
                        ),
                    )
                    .child(button("mock-table-ok", "应用", ActionKind::Main).on_click(
                        move |_, window, app| {
                            let applied = ok_panel.update(app, |panel, cx| {
                                panel.apply_table_edit(cx);
                                panel.table_editing.is_none()
                            });
                            if applied {
                                window.close_dialog(app);
                            }
                        },
                    )),
            )
        });
    }

    /// 应用「编辑表」：校验 → 改工作副本 → **同步指向它的关系的 `ref_table`**。
    pub fn apply_table_edit(&mut self, cx: &mut Context<Self>) {
        let Some(original) = self.table_editing.clone() else {
            return;
        };
        // 表名走与单表路径**同一个**校验（非空 / 字母数字下划线 / 不以数字开头）：
        // 让非法名在这里就被拦住，而不是到生成时才由引擎报出来
        let name = match self
            .edit_name_input
            .as_ref()
            .map(|input| validate_table_name(&input.read(cx).value()))
        {
            Some(Ok(name)) => name,
            Some(Err(e)) => {
                self.fail(e, cx);
                return;
            }
            None => return,
        };
        let rows = match self
            .edit_rows_input
            .as_ref()
            .map(|input| parse_rows(&input.read(cx).value()))
        {
            Some(Ok(rows)) => rows,
            Some(Err(e)) => {
                self.fail(e, cx);
                return;
            }
            None => return,
        };
        let renamed = name != original;
        let Some(template) = self.scenario.as_mut() else {
            return;
        };
        if renamed && template.tables.iter().any(|t| t.name == name) {
            self.fail(format!("本次生成里已经有表 {name}"), cx);
            return;
        }
        let Some(table) = template.tables.iter_mut().find(|t| t.name == original) else {
            return;
        };
        table.name = name.clone();
        table.row_count = rows;
        let mut retargeted = 0usize;
        if renamed {
            // 改了名，指向它的引用必须跟着改——否则立即变成“指向模板里没有的表”
            for child in template.tables.iter_mut() {
                for column in child.columns.iter_mut() {
                    let points_here = column.dependency.as_mut().is_some_and(|dep| {
                        if dep.ref_table == original.as_str() {
                            dep.ref_table = name.clone();
                            true
                        } else {
                            false
                        }
                    });
                    if points_here {
                        retargeted += 1;
                    }
                }
            }
        }
        self.table_editing = None;
        self.error = None;
        self.outcome = Some(if retargeted == 0 {
            format!(
                "已更新表 {name}（{} 行）：下次生成生效",
                with_thousands(u64::from(rows))
            )
        } else {
            format!(
                "已更新表 {name}（{} 行），并同步 {retargeted} 条指向它的引用：下次生成生效",
                with_thousands(u64::from(rows))
            )
        });
        cx.notify();
    }

    /// 打开「加关系」对话框（子表.列 → 父表.列）。
    ///
    /// 父列只列**自增列**：只有它能算出取值域，其他列在引擎侧会被拒——
    /// 把不能选的东西藏起来，比先让用户选完再报错好。
    pub fn open_relation_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.clone() else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        // 每次打开都从「未选」开始：上一次的选择带到下一次容易误改
        *self.relation_pick.borrow_mut() = RelationPick::default();
        let pick = self.relation_pick.clone();
        let panel = cx.entity();
        let template = Rc::new(template);

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let current = pick.borrow().clone();
            let table_names: Vec<String> = template.tables.iter().map(|t| t.name.clone()).collect();
            let child_columns: Vec<String> = current
                .child_table
                .as_deref()
                .and_then(|name| template.tables.iter().find(|t| t.name == name))
                .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            let parent_columns: Vec<String> = current
                .parent_table
                .as_deref()
                .and_then(|name| template.tables.iter().find(|t| t.name == name))
                .map(|t| {
                    t.columns
                        .iter()
                        .filter(|c| matches!(c.generator, GeneratorConfig::AutoIncrement { .. }))
                        .map(|c| c.name.clone())
                        .collect()
                })
                .unwrap_or_default();

            // 一个下拉行：标签 + 当前值按钮 + 菜单
            let row = |id: &'static str,
                       label: &'static str,
                       current: Option<&String>,
                       placeholder: &'static str,
                       items: Vec<String>,
                       empty_note: &'static str,
                       on_pick: Rc<dyn Fn(String, &mut App)>| {
                let pick_for_menu = on_pick.clone();
                let text = match current {
                    Some(value) => value.clone(),
                    None => placeholder.to_string(),
                };
                // 走 `kv` 原语：标签在左、控件贴右端，四个下拉的触发器右端对齐。
                // 此前标签定宽 `rems(4.0)`、控件紧跟标签——「子列」与「父表」两个词长度不同，
                // 但触发器起点一样，看起来像两列又对不齐。
                kv(
                    theme,
                    label,
                    dropdown_button(id, text)
                        // caret 由 `dropdown_button` 统一挂（此前手拼「▾」字形，
                        // 部分字体下会掉成豆腐块）
                        .dropdown_menu(move |menu, _window, _cx| {
                            let mut menu = menu;
                            if items.is_empty() {
                                return menu.item(PopupMenuItem::new(empty_note).disabled(true));
                            }
                            for item in items.iter() {
                                let value = item.clone();
                                let pick_for_item = pick_for_menu.clone();
                                menu =
                                    menu.item(PopupMenuItem::new(item.clone()).on_click(
                                        move |_, _, app| pick_for_item(value.clone(), app),
                                    ));
                            }
                            menu
                        }),
                )
            };

            // 每个下拉的点击：改工作选择，再请面板重绘（对话框由窗口重绘时重建）
            let pick_child_table = pick.clone();
            let panel_child_table = panel.clone();
            let on_child_table = Rc::new(move |value: String, app: &mut App| {
                let mut pick = pick_child_table.borrow_mut();
                pick.child_table = Some(value);
                pick.child_column = None;
                drop(pick);
                panel_child_table.update(app, |_, cx| cx.notify());
            });
            let pick_child_column = pick.clone();
            let panel_child_column = panel.clone();
            let on_child_column = Rc::new(move |value: String, app: &mut App| {
                pick_child_column.borrow_mut().child_column = Some(value);
                panel_child_column.update(app, |_, cx| cx.notify());
            });
            let pick_parent_table = pick.clone();
            let panel_parent_table = panel.clone();
            let on_parent_table = Rc::new(move |value: String, app: &mut App| {
                let mut pick = pick_parent_table.borrow_mut();
                pick.parent_table = Some(value);
                pick.parent_column = None;
                drop(pick);
                panel_parent_table.update(app, |_, cx| cx.notify());
            });
            let pick_parent_column = pick.clone();
            let panel_parent_column = panel.clone();
            let on_parent_column = Rc::new(move |value: String, app: &mut App| {
                pick_parent_column.borrow_mut().parent_column = Some(value);
                panel_parent_column.update(app, |_, cx| cx.notify());
            });

            let ready = current.child_table.is_some()
                && current.child_column.is_some()
                && current.parent_column.is_some();
            let mut body = div()
                .v_flex()
                .gap_2()
                .child(hint_line(
                    theme,
                    "引用只在本次多表生成内成立：值取自父表主键的取值域（由自增参数与行数算出）。",
                ))
                .child(hint_line(
                    theme,
                    "不读已落地的数据，也不修改已生成的表 / 文件。",
                ))
                .child(card_divider(theme))
                .child(row(
                    "mock-relation-child-table",
                    "子表",
                    current.child_table.as_ref(),
                    "选择子表",
                    table_names.clone(),
                    "（模板里没有表）",
                    on_child_table,
                ))
                .child(row(
                    "mock-relation-child-column",
                    "子列",
                    current.child_column.as_ref(),
                    "选择子列",
                    child_columns,
                    "（先选子表）",
                    on_child_column,
                ))
                .child(row(
                    "mock-relation-parent-table",
                    "父表",
                    current.parent_table.as_ref(),
                    "选择父表",
                    table_names,
                    "（模板里没有表）",
                    on_parent_table,
                ))
                .child(row(
                    "mock-relation-parent-column",
                    "父列",
                    current.parent_column.as_ref(),
                    "选择父列",
                    parent_columns.clone(),
                    "（先选父表）",
                    on_parent_column,
                ));
            if current.parent_table.is_some() && parent_columns.is_empty() {
                body = body.child(card_divider(theme)).child(warning_line(
                    theme,
                    "这张表没有自增主键列——引用目标只能是自增列（域算得出才不用去读数据）",
                ));
            }

            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            dialog.title("加表间关系").child(body).footer(
                DialogFooter::new()
                    .child(
                        button("mock-relation-cancel", "取消", ActionKind::Soft).on_click(
                            move |_, window, app| {
                                let _ = cancel_panel.update(app, |panel, cx| {
                                    *panel.relation_pick.borrow_mut() = RelationPick::default();
                                    cx.notify();
                                });
                                window.close_dialog(app);
                            },
                        ),
                    )
                    .child({
                        let mut ok =
                            button("mock-relation-ok", "添加", ActionKind::Main).disabled(!ready);
                        if ready {
                            ok = ok.on_click(move |_, window, app| {
                                let _ = ok_panel.update(app, |panel, cx| panel.add_relation(cx));
                                window.close_dialog(app);
                            });
                        }
                        ok
                    }),
            )
        });
    }

    /// 把对话框里选好的四个位置写成一条关系（挂在子列上）。
    ///
    /// 同时把该列的 `generator` **对齐到父域**：单表生成时按它取值，
    /// 域必须落在父域内（否则同一列换个入口就产出不一样的值）。
    pub(crate) fn add_relation(&mut self, cx: &mut Context<Self>) {
        let pick = self.relation_pick.borrow().clone();
        let (Some(child_table), Some(child_column), Some(parent_table), Some(parent_column)) = (
            pick.child_table,
            pick.child_column,
            pick.parent_table,
            pick.parent_column,
        ) else {
            self.fail("子表 / 子列 / 父表 / 父列都要选", cx);
            return;
        };

        let mut problem: Option<String> = None;
        let mut message = String::new();
        if let Some(template) = self.scenario.as_mut() {
            let domain = template
                .tables
                .iter()
                .find(|t| t.name == parent_table)
                .and_then(|parent| {
                    let column = parent.columns.iter().find(|c| c.name == parent_column)?;
                    let GeneratorConfig::AutoIncrement { start, step } = column.generator else {
                        return None;
                    };
                    Some(ReferenceDomain {
                        table: parent_table.clone(),
                        column: parent_column.clone(),
                        first: i64::from(start),
                        step: i64::from(step),
                        count: parent.row_count,
                    })
                });
            match domain {
                None => {
                    problem = Some(format!(
                        "{parent_table}.{parent_column} 不能作引用目标（只支持自增主键列）"
                    ));
                }
                Some(domain) => {
                    let child = template
                        .tables
                        .iter_mut()
                        .find(|t| t.name == child_table)
                        .and_then(|t| t.columns.iter_mut().find(|c| c.name == child_column));
                    match child {
                        None => {
                            problem = Some(format!("子表 {child_table} 没有列 {child_column}"));
                        }
                        Some(column) => {
                            column.dependency =
                                Some(ColumnDependency::referenced(&parent_table, &parent_column));
                            column.generator = GeneratorConfig::RandomInt {
                                min: domain.first as i32,
                                max: domain.last() as i32,
                            };
                            message = format!(
                                "已加关系 {child_table}.{child_column} → {}",
                                domain.label()
                            );
                        }
                    }
                }
            }
        }
        // 提交前再校一遍：同一列被指向两张表之类的问题在这里就能看出来。
        // 不覆盖已有问题：用户刚做的动作报出来的原因（父列不可用）比它的下游后果更直接。
        if problem.is_none() {
            let check = match self.scenario.as_ref() {
                Some(template) => MockEngine::resolve_reference_domains(template).err(),
                None => None,
            };
            if let Some(e) = check {
                problem = Some(format!("表间关系有问题：{e}"));
            }
        }
        *self.relation_pick.borrow_mut() = RelationPick::default();
        match problem {
            Some(problem) => self.fail(problem, cx),
            None => self.succeed(message, cx),
        }
    }

    /// 删一条关系（只拆引用；该列的生成器保留，它现在是普通随机列）。
    pub fn remove_relation(
        &mut self,
        child_table: &str,
        child_column: &str,
        cx: &mut Context<Self>,
    ) {
        let mut removed = false;
        if let Some(template) = self.scenario.as_mut() {
            if let Some(column) = template
                .tables
                .iter_mut()
                .find(|t| t.name == child_table)
                .and_then(|t| t.columns.iter_mut().find(|c| c.name == child_column))
            {
                removed = column.dependency.take().is_some();
            }
        }
        if removed {
            self.outcome = Some(format!("已删除关系 {child_table}.{child_column}"));
            self.error = None;
            cx.notify();
        }
    }
}
