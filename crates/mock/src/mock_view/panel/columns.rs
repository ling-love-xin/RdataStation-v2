// ==================== 列操作 ====================

use super::*;

impl MockPanel {
    /// 手工加列（按类型给默认生成器）。
    pub fn add_column(&mut self, name: String, data_type: ColumnDataType, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let spec = new_column_spec(id, name, data_type);
        self.draft.columns.push(spec);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 从字段预设加列（列名冲突自动加序号：`name` → `name_2`）。
    /// 应用后是普通列，照常走列编辑对话框微调，不锁定后续编辑。
    pub fn add_preset_column(&mut self, preset: &FieldPreset, cx: &mut Context<Self>) {
        let base = preset.column.name.clone();
        let mut name = base.clone();
        let mut n = 2u32;
        while self.draft.columns.iter().any(|c| c.def.name == name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        let spec = MockColumnSpec {
            id,
            def: ColumnDef {
                name,
                ..preset.column.clone()
            },
            confidence: preset.confidence.to_string(),
            sample_value: String::new(),
            user_generator: None,
        };
        self.draft.columns.push(spec);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 删列。
    pub fn remove_column(&mut self, id: u64, cx: &mut Context<Self>) {
        self.draft.columns.retain(|c| c.id != id);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 调整列顺序（↑/↓）：列序 = 生成顺序与结果表列序，多列场景需要能改。
    /// 目标位置越界（已在首/尾）时是 no-op，由调用方控制按钮显隐即可。
    pub fn move_column(&mut self, id: u64, up: bool, cx: &mut Context<Self>) {
        let index = match self.draft.columns.iter().position(|c| c.id == id) {
            Some(index) => index,
            None => return,
        };
        let target = if up {
            // 首列不能再上移：no-op（不要 `index - 1` 下溢）
            if index == 0 {
                return;
            }
            index - 1
        } else {
            index + 1
        };
        if target >= self.draft.columns.len() {
            return;
        }
        self.draft.columns.swap(index, target);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 复制列（深拷贝配置，新 id，名字加「副本」后缀，插在原列后面）。
    /// 用于快速造相似字段：复制后再微调参数，比重新加列 + 重配快。
    pub fn duplicate_column(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.draft.columns.iter().position(|c| c.id == id) else {
            return;
        };
        let mut copy = self.draft.columns[index].clone();
        copy.id = self.next_id;
        self.next_id += 1;
        copy.def.name = format!("{} 副本", copy.def.name);
        self.draft.columns.insert(index + 1, copy);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 直接换生成器（取规格默认参数）——详情 tab 的生成器菜单。
    pub fn set_generator(&mut self, id: u64, name: &str, cx: &mut Context<Self>) {
        let Some(config) = generator_catalog::default_of(name) else {
            return;
        };
        self.remember_generator(name, false);
        if let Some(column) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            column.def.generator = config;
            column.confidence = "manual".to_string();
            column.sample_value.clear();
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 字段行入口两栏模式「应用」：写整份生成器配置 + 列级开关（空值率 / 唯一值）到指定列。
    ///
    /// 与 [`set_generator`]（只取默认配置、不含列级开关）区分——两栏右栏允许在落地前调
    /// 空值率 / 唯一值，这里一次写齐。`remember_generator` + `manual` 置信度 + 清空结果集同口径。
    pub fn set_generator_config(
        &mut self,
        id: u64,
        config: GeneratorConfig,
        nullable_ratio: f64,
        unique: bool,
        cx: &mut Context<Self>,
    ) {
        self.remember_generator(generator_catalog::spec_of(&config).name, false);
        if let Some(column) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            column.def.generator = config;
            column.def.nullable_ratio = nullable_ratio;
            column.def.unique = unique;
            column.confidence = "manual".to_string();
            column.sample_value.clear();
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 本会话点过的生成器（最近在前；生成器菜单的「最近使用」，见 B7）。
    ///
    /// 记的是 `(键, 是否自定义)`：内建键 = 目录名（`spec.name`），自定义键 = `ug.id`。
    /// 只存目录名会让自定义生成器永远进不了历史（它没有目录名）。
    pub fn recent_generators(&self) -> &[(String, bool)] {
        &self.recent_generators
    }

    /// 某列的**推荐生成器**（目录名）——生成器菜单里标「推荐」（B7）。
    ///
    /// 与导入结构走同一条推理（`ColumnMapper::infer`：列名 + 类型 → 生成器），
    /// 但**只用来标记菜单项**，不写回配置：写回仍然只有用户点击（或导入那条路径）。
    pub fn recommended_generator(&self, name: &str, data_type: &ColumnDataType) -> String {
        generator_catalog::spec_of(&ColumnMapper::infer(name, data_type).generator)
            .name
            .to_string()
    }

    /// 记一笔「刚用过哪个生成器」（去重后放最前，只留 [`RECENT_GENERATORS`] 条）。
    ///
    /// `key` 与写回用的键同口径（内建 = `spec.name`，自定义 = `ug.id`）：历史 chips 要按它
    /// 反查并写回，拿显示名记就再也对不上了。
    pub(crate) fn remember_generator(&mut self, key: &str, is_user: bool) {
        self.recent_generators.retain(|(recent, _)| recent != key);
        self.recent_generators.insert(0, (key.to_string(), is_user));
        self.recent_generators.truncate(RECENT_GENERATORS);
    }

    /// 打开「搜索生成器」对话框（143 项按名称 / 中文标签 / 分类过滤）。
    ///
    /// 用 `List` 组件而不是手搜：搜索框 / 虚拟化 / 上下键 / 回车与点击确认 / 空态全是组件的，
    /// 且搜索是**同步**的（目录全在内存），不会闪 loading。
    pub fn open_generator_search(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        // 当前生成器从自身状态取：此刻面板正在被更新，不能再 `read` 自己
        let current = self
            .draft
            .columns
            .iter()
            .find(|column| column.id == id)
            .map(|column| generator_catalog::spec_of(&column.def.generator).name);
        let delegate = GeneratorSearchDelegate::new(cx.entity(), id, current);
        // `searchable` 在 `ListState` 上（搜索框是状态的一部分），占位文案在元素上
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("搜索生成器")
                .child(div().w_full().h(rems(ui::SEARCH_LIST_HEIGHT)).child(
                    List::new(&list).search_placeholder("按名称 / 中文标签 / 分类搜索（143 项）"),
                ))
        });
    }

    /// 应用列编辑（列名 / 类型 / 生成器 / 参数 / 空值率 / 唯一）——详情 tab 的「应用」。
    pub fn apply_column(&mut self, edited: MockColumnSpec, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == edited.id) {
            *slot = edited;
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    // ==================== 自定义生成器（分组「＋」精调层创建；会话级，持久化记录在册） ====================

    /// 本会话全部自定义生成器（生成器菜单 / 库对话框共用）。
    pub(crate) fn user_generators(&self) -> &[UserGenerator] {
        &self.user_generators
    }

    /// 业务视角：该分类下的自定义生成器（没选业务组的只挂技术组，这里不列）。
    pub(crate) fn user_generators_in_category(
        &self,
        category: GeneratorCategory,
    ) -> Vec<&UserGenerator> {
        self.user_generators
            .iter()
            .filter(|ug| ug.category == Some(category))
            .collect()
    }

    /// 技术视角：该数据类下的自定义生成器（类型是硬属性，永远挂）。
    pub(crate) fn user_generators_in_type(
        &self,
        data_type: &ColumnDataType,
    ) -> Vec<&UserGenerator> {
        self.user_generators
            .iter()
            .filter(|ug| &ug.data_type == data_type)
            .collect()
    }

    /// 新增自定义生成器（命名变体：已有生成器 + 参数 + 名称）。
    ///
    /// 名称必填；与既有自定义生成器（含内建生成器目录名？**不冲突**——菜单里自定义区与目录
    /// 分开渲染，`id` 只在本会话自定义集合内去重）重名自动加序号（`name` → `name_2` → …，
    /// 沿用 §8.3 字段预设的命名策略）。返回最终 id（去重后）。
    pub(crate) fn add_user_generator(
        &mut self,
        name: &str,
        data_type: ColumnDataType,
        category: Option<GeneratorCategory>,
        base: GeneratorConfig,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("生成器名不能为空".to_string());
        }
        let mut id = trimmed.to_string();
        let mut seq = 2;
        while self.user_generators.iter().any(|ug| ug.id == id) {
            id = format!("{trimmed}_{seq}");
            seq += 1;
        }
        self.user_generators.push(UserGenerator {
            id: id.clone(),
            name: id.clone(),
            data_type,
            category,
            base,
        });
        cx.notify();
        Ok(id)
    }

    /// 更新自定义生成器（编辑保存：改名 + 全量替换 base / 分组）。
    ///
    /// `id` 是编辑入口传入的旧 id；改名后重新去重（排除自己），返回最终 id。
    pub(crate) fn update_user_generator(
        &mut self,
        id: &str,
        name: &str,
        data_type: ColumnDataType,
        category: Option<GeneratorCategory>,
        base: GeneratorConfig,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("生成器名不能为空".to_string());
        }
        let mut new_id = trimmed.to_string();
        let mut seq = 2;
        while self
            .user_generators
            .iter()
            .any(|ug| ug.id == new_id && ug.id != id)
        {
            new_id = format!("{trimmed}_{seq}");
            seq += 1;
        }
        let Some(ug) = self.user_generators.iter_mut().find(|ug| ug.id == id) else {
            return Err("找不到该自定义生成器".to_string());
        };
        ug.id = new_id.clone();
        ug.name = new_id.clone();
        ug.data_type = data_type;
        ug.category = category;
        ug.base = base;
        cx.notify();
        Ok(new_id)
    }

    /// 删除自定义生成器（会话级）。已应用该生成器的列不受影响——
    /// 应用写回的是解包后的 base（普通列配置），与自定义项解耦。
    pub(crate) fn delete_user_generator(&mut self, id: &str, cx: &mut Context<Self>) {
        self.user_generators.retain(|ug| ug.id != id);
        cx.notify();
    }

    /// 应用自定义生成器：把 `base`（已有生成器 + 参数）解包写回列，confidence=manual。
    ///
    /// 与 `set_generator` 的区别：目录里没有自定义名（它不在 `generator_catalog`），
    /// 不能经 `default_of(name)` 查——直接从 `user_generators` 取打包配置。
    /// 生成链路零改动：写回后列配置就是普通列，`generate_cell` 照常跑。
    pub fn apply_user_generator(&mut self, id: u64, ug_id: &str, cx: &mut Context<Self>) {
        let Some(ug) = self
            .user_generators
            .iter()
            .find(|ug| ug.id == ug_id)
            .cloned()
        else {
            return;
        };
        if let Some(column) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            column.def.generator = ug.base;
            column.confidence = "manual".to_string();
            column.sample_value.clear();
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 按列名 + 类型重跑智能映射（「恢复智能默认」）。
    pub fn reset_column_mapping(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            let mapped = ColumnMapper::infer(&slot.def.name, &slot.def.data_type);
            slot.def.generator = mapped.generator;
            slot.def.nullable_ratio = 0.0;
            slot.def.unique = false;
            slot.confidence = mapped.confidence;
            slot.sample_value = mapped.sample_value;
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }
}
