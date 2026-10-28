# rds-file-meta（文件元信息）设计文档

> 类型：设计 / 实现文档。对应 crate：`crates/file-meta`（`rds-file-meta`）。
> 入口指针见 `docs/architecture/file-meta/README.md` 与 `crates/file-meta/README.md`。

## 一、背景与定位

文件元信息管理系统，记录每个文件**快照式**的元信息。

- 编辑器、草稿箱、资源管理器等模块都需要记录文件的附加信息
- 这些信息和文件绑定，文件移动了，元信息也要跟着走
- 不适合放在项目设置里（因为文件可能跨项目移动）

## 二、核心需求

### 1. Sidecar 文件模式

- 每个文件对应一个元信息文件：`query.sql` → `.query.sql.meta.json`
- **默认**：元信息文件和源文件同目录（`.` 前缀隐藏，用户默认看不到）；文件移动时同目录 sidecar 跟着走
- **可自定义目录**：`read_meta_at` / `write_meta_at` 接受显式 sidecar 路径，供把元信息集中放到隐藏目录的消费方
  （如草稿箱用 `.RSmeta/scratchpad/meta/`），避免污染资源管理、天然避开文件监控自激

### 2. 快照式元信息

只记录当前状态，不记录历史（拉链式的不要）：

- ✅ 记录「当前有哪些书签」，不记录「书签是怎么变化的」
- ✅ 记录「上次连接的是哪个数据库」，不记录「连接历史」

### 3. 可扩展

- 核心结构要稳定，以后加新字段不用大改
- 留一个 `extra` 字段，支持任意扩展属性（消费方把自己的专属字段放进去，不占公共字段）

## 三、功能列表

### P0（已实现）

| 功能 | 说明 |
| --- | --- |
| 元信息读写 | 读取 / 写入 `.filename.meta.json`（`read_meta` / `write_meta`） |
| 显式路径读写 | `read_meta_at` / `write_meta_at`（自定义目录；`write_meta_at` 自动建父目录） |
| 书签管理 | `add_bookmark` / `remove_bookmark` / `list_bookmarks` |
| 默认值 | 元信息文件不存在时，返回默认空元信息 |
| 清理函数 | `cleanup_meta`：原文件不存在时，自动删除元信息文件 |
| 结构版本号 | `schema_version`（当前 `CURRENT_SCHEMA_VERSION = 1`），以后迁移用 |

### P1（数据结构已落地，写读往返可用）

| 功能 | 说明 |
| --- | --- |
| 执行元信息 | 上次连接、执行模式、上次执行行号（`ExecutionMeta`） |
| 视图元信息 | 折叠状态、字体大小（`ViewMeta` / `FoldRange`） |
| 标签 | 引用另一个 `tag` crate，不要自己实现 |

## 四、数据结构

```rust
/// 当前结构版本号（以后升级结构时递增，并配合迁移逻辑）
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

pub struct FileMeta {
    /// 结构版本号（以后迁移用）
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// 书签列表（行号从 1 开始）
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    /// 上次连接 ID（快照：只记当前连的是哪个，不记连接历史）
    #[serde(default)]
    pub last_connection: Option<String>,
    /// 执行相关元信息
    #[serde(default)]
    pub execution: ExecutionMeta,
    /// 视图相关元信息
    #[serde(default)]
    pub view: ViewMeta,
    /// 扩展属性（以后加新属性不用改结构）
    #[serde(default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

pub struct Bookmark {
    pub line: usize,      // 行号（从 1 开始）
    pub label: String,    // 说明文字
}

pub struct ExecutionMeta {
    pub mode: String,           // 执行模式（current / all / selection）
    pub last_line: Option<usize>, // 上次执行行号
}

pub struct ViewMeta {
    pub folded_ranges: Vec<FoldRange>, // 折叠范围
    pub font_size: Option<f32>,         // 字体大小
}

pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
}
```

> `FileMeta` 的 `Default` 把 `schema_version` 落为 `CURRENT_SCHEMA_VERSION`（不靠 derive 给 0）；
> 所有顶层字段都带 `#[serde(default)]`，旧格式 / 缺字段宽容反序列化（为以后迁移兜底）。
> 读取到**高于当前版本**的 `schema_version` 时 `tracing::warn` 降级读取（前向兼容提示）。

## 五、API

| 函数 | 说明 |
| --- | --- |
| `meta_path(file_path) -> PathBuf` | 同目录 sidecar 路径：`query.sql` → `.query.sql.meta.json` |
| `read_meta(file_path) -> Result<FileMeta>` | 读同目录 sidecar；缺失返回默认空元信息 |
| `read_meta_at(meta_path) -> Result<FileMeta>` | 读**显式路径** sidecar；缺失返回默认；更高 schema 降级告警 |
| `write_meta(file_path, &meta)` | 写同目录 sidecar |
| `write_meta_at(meta_path, &meta)` | 写**显式路径** sidecar；父目录不存在时自动创建 |
| `cleanup_meta(file_path)` | 原文件不存在时自动删除同目录 sidecar；否则空操作 |
| `add_bookmark / remove_bookmark / list_bookmarks` | 书签读写的便捷封装（基于同目录 sidecar，read-modify-write） |

错误统一为 `MetaError`（IO / JSON）。

## 六、注意事项

1. **tag 是另一个 crate**：不要在 file-meta 里实现 tag 功能。需要时引用 `crates/tag`（`rds-tag`），或留接口以后接。
2. **快照式，不要历史**：不记录执行 / 打开 / 连接历史，只记录「当前是什么状态」。
3. **元信息文件命名**：默认 `.{filename}.meta.json`（同目录，`.` 前缀隐藏）。消费方可改用 `read_meta_at` / `write_meta_at` 放自定义隐藏目录（如草稿箱的 `.RSmeta/scratchpad/meta/`）。
4. **并发问题**：多窗口同时打开同一文件时不考虑同步，简单方案为「最后写入胜出」；以后有问题再加文件锁。
5. **行号漂移**：**已实现（方案 A）**。内核在文本变更时记录编辑行增量 `LineDelta`，编辑器收到 `InputEvent::Change` 后 `take_last_edit_delta()` 平移书签（编辑区前不动 / 区内锚到编辑区首行 / 区后整体平移），与 VSCode 书签插件一致。内核改动登记在 `docs/architecture/ui/gpui-kit-0.8.0-upgrade-report.md` §3 ⑥（升级 gpui-kit 时按清单重打，不丢）。

## 七、消费方

| 消费方 | 用途 | 位置策略 | API |
| --- | --- | --- | --- |
| `crates/editor` | 书签读写 + 偏移校正（方案 A） | 同目录 | `read_meta` / `write_meta` / `LineDelta` |
| `crates/scratchpad` | 草稿连接元信息迁移（第③步） | 集中隐藏目录 `.RSmeta/scratchpad/meta/` | `read_meta_at` / `write_meta_at` |

## 八、实现状态

- [x] P0 全部实现：元信息读写、显式路径读写（`_at`）、书签管理、默认值、清理函数 `cleanup_meta()`、`schema_version`
- [x] P1 数据结构（`ExecutionMeta` / `ViewMeta` / `FoldRange`）已落在 `FileMeta` 里，写读往返可用（12 个单测全绿）
- [x] 行号漂移校正（方案 A）：内核 `LineDelta` + 编辑器书签平移（2026-10-28）
- [x] 草稿箱连接元信息迁移（第③步，2026-10-28）：scratchpad 用 `_at` API 落到集中隐藏目录，字段走 `extra`（`scratchpad.bound_connections` / `scratchpad.last_executed_at`）
- [ ] 标签（tag）：按约定不在此 crate 实现，后续接入 `crates/tag`（`rds-tag`）或留接口
