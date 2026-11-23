//! 数据对象「工作集」的按文档落盘。
//!
//! 工作集 = 用户勾进「本次分析要用的对象」的那几个（`Object::key`）。它是**面板的偏好**，
//! 而且跟文档走：同一份 SQL 里勾了哪几张表，切到另一份文档不该原样搬过去。
//!
//! ## 落点与理由
//!
//! 落在 `{项目}/.RSmeta/workbench/working_set.json`（与草稿箱元数据同一层目录，
//! 见 `services::scratchpad_meta`）：
//!
//! - **不进 `editor_contexts`**：那张表是编辑器会话（光标 / 模式 / 内容 / 结果列布局）的
//!   归处，编辑器不认识 `srctable:C_1|db|main|t` 这种数据对象 key；硬塞进去会让
//!   「会话」和「面板偏好」两套语义纠缠在同一行，各自都不知道对方在改什么。
//! - **不进全局库**：工作集是项目内的东西（勾的是这个项目库里看到的表），
//!   换项目就该是另一份；放全局库反而要额外造一个「按项目 × 按文档」的键。
//! - **不进 `.rds/tmp/`**：那是过程产物（可随时清），工作集是用户的选择，清了会心疼。
//!
//! ## 什么文档会被存
//!
//! 只存**有路径的文档**（`DocumentId` 由路径派生，见 `workbench_shell::model::DocumentId`
//! 的注释）：未命名文档的 id 是运行时序号（`doc-7`），重启后指向的是另一份空白文档，
//! 存下来只会把「某份没名字的文档」恢复成别人的选择（与编辑器会话「未命名不落库」同一条口径）。
//!
//! ## 坏数据策略
//!
//! 读：文件不在 / JSON 坏 / 字段不认识 → 当空表（**不报错**，工作集丢一份不影响能用）。
//! 写：先写临时文件再换名（中途崩不会留下半截 JSON）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 相对项目根的落点（与 `services::scratchpad_meta` 同级目录）
pub const REL: &str = ".RSmeta/workbench/working_set.json";

/// 磁盘形态
///
/// `version` 留着是为了以后换编码时有话可说（现在是 1）；不认的版本当空表读。
#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    version: u32,
    /// 文档 key → 该文档的工作集（`Object::key` 列表）
    #[serde(default)]
    sets: BTreeMap<String, Vec<String>>,
}

/// 当前认识的版本号
const VERSION: u32 = 1;

/// 落盘路径（`None` 项目根 = 没有项目，不落盘）
pub fn file_path(project_root: &Path) -> PathBuf {
    project_root.join(REL)
}

/// 文档能不能落盘：只认「像路径」的 id（含分隔符的才算有路径的文档）。
///
/// 为什么不用「查一下这份文档有没有 path」：面板只拿得到 `DocumentId`，
/// 而 `DocumentId` 对文件文档正是由路径派生（另存为不换身份），未命名是 `doc-N`。
/// 用形状判比再引一条跨 crate 查询更稳（也不会把 `doc-3` 误当成一份真文档）。
pub fn doc_key(id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() || !(id.contains('/') || id.contains('\\')) {
        return None;
    }
    Some(id.to_string())
}

/// 读整份表（读不到 = 空表）
pub fn load(project_root: Option<&Path>) -> BTreeMap<String, Vec<String>> {
    let Some(root) = project_root else {
        return BTreeMap::new();
    };
    let path = file_path(root);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    match serde_json::from_str::<OnDisk>(&text) {
        Ok(disk) if disk.version == VERSION => disk.sets,
        Ok(disk) => {
            tracing::warn!(
                path = %path.display(),
                version = disk.version,
                "工作集文件的版本不认识：按空表读（不猜旧格式）"
            );
            BTreeMap::new()
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "工作集文件读不了：按空表读");
            BTreeMap::new()
        }
    }
}

/// 写整份表（空表 = 删文件，不留一个空壳）
pub fn save(
    project_root: Option<&Path>,
    sets: &BTreeMap<String, Vec<String>>,
) -> Result<(), String> {
    let Some(root) = project_root else {
        return Ok(());
    };
    let path = file_path(root);
    // 只落有路径的文档、只落非空的清单：空条目存下来等于给每份文档留个假记录
    let kept: BTreeMap<String, Vec<String>> = sets
        .iter()
        .filter(|(id, keys)| doc_key(id).is_some() && !keys.is_empty())
        .map(|(id, keys)| (id.clone(), keys.clone()))
        .collect();
    if kept.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| {
                format!("删掉空的工作集文件失败（{}）：{error}", path.display())
            })?;
        }
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("建工作集目录失败（{}）：{error}", dir.display()))?;
    }
    let disk = OnDisk {
        version: VERSION,
        sets: kept,
    };
    let text = serde_json::to_string_pretty(&disk)
        .map_err(|error| format!("工作集序列化失败：{error}"))?;
    // 先写临时文件再换名：写一半崩掉不会留下半截 JSON（下次读就是空表）
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)
        .map_err(|error| format!("写工作集临时文件失败（{}）：{error}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .map_err(|error| format!("替换工作集文件失败（{}）：{error}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{doc_key, file_path, load, save};
    use std::collections::BTreeMap;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_ws_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn unnamed_documents_are_not_persistable() {
        // 未命名文档的 id 是运行时序号：重启后指到的是另一份空白文档
        assert_eq!(doc_key("doc-7"), None);
        assert_eq!(doc_key(""), None);
        assert_eq!(doc_key("   "), None);
        // 有路径的文档（Windows / POSIX 两种写法都认）
        assert_eq!(doc_key("d:/sql/a.sql").as_deref(), Some("d:/sql/a.sql"));
        assert_eq!(
            doc_key(r"D:\sql\a.sql").as_deref(),
            Some(r"D:\sql\a.sql")
        );
    }

    #[test]
    fn a_working_set_round_trips_per_document() {
        let root = temp_root("roundtrip");
        let mut sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        sets.insert(
            "d:/sql/a.sql".to_string(),
            vec!["asset:res_1".to_string(), "srctable:C_1|db|main|t".to_string()],
        );
        sets.insert("d:/sql/b.sql".to_string(), vec!["result:tmp_2".to_string()]);
        // 未命名文档也在表里（运行期内存里有它），但**不该落盘**
        sets.insert("doc-3".to_string(), vec!["asset:res_9".to_string()]);

        save(Some(&root), &sets).expect("save");
        let back = load(Some(&root));
        assert_eq!(back.len(), 2, "未命名文档被过滤，不进文件");
        assert_eq!(
            back.get("d:/sql/a.sql").map(|v| v.as_slice()),
            Some(["asset:res_1".to_string(), "srctable:C_1|db|main|t".to_string()].as_slice())
        );
        assert_eq!(
            back.get("d:/sql/b.sql").map(|v| v.as_slice()),
            Some(["result:tmp_2".to_string()].as_slice())
        );
        assert!(file_path(&root).exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_table_leaves_no_file_behind() {
        let root = temp_root("empty");
        let mut sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        sets.insert("d:/sql/a.sql".to_string(), vec!["asset:res_1".to_string()]);
        save(Some(&root), &sets).expect("save one");
        assert!(file_path(&root).exists());

        // 最后一个工作集被清空 → 删文件，不留空壳（下次读也就是空表）
        save(Some(&root), &BTreeMap::new()).expect("save empty");
        assert!(!file_path(&root).exists(), "空表不该留文件");
        assert!(load(Some(&root)).is_empty());
        // 删已经不在的文件也不该报错（幂等）
        save(Some(&root), &BTreeMap::new()).expect("save empty again");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_broken_file_reads_as_an_empty_table_instead_of_panicking() {
        let root = temp_root("broken");
        let path = file_path(&root);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "{ not json").expect("write broken");
        assert!(load(Some(&root)).is_empty());

        // 不认识的版本：同样按空表读（不猜旧格式，也不把项目里的文件删掉）
        std::fs::write(&path, r#"{"version": 9, "sets": {"d:/a.sql": ["asset:res_1"]}}"#)
            .expect("write unknown version");
        assert!(load(Some(&root)).is_empty());
        assert!(path.exists(), "读不动不等于该删");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn without_a_project_nothing_is_read_or_written() {
        // 没项目 = 没有落点：读是空表，写是成功但什么都不做
        assert!(load(None).is_empty());
        let mut sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        sets.insert("d:/sql/a.sql".to_string(), vec!["asset:res_1".to_string()]);
        save(None, &sets).expect("no-op save");
    }
}
