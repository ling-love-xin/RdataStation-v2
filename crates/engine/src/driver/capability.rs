//! 驱动能力字典（**一本**）：能力键 → 中文标签 +（可选的）运行时能力位 + 真机验收状态。
//!
//! ## 为什么要有这本字典
//!
//! 同一个数据库可以有多个驱动实现（`mysql` 的 sqlx 版与 Official 版、`postgres` 的两个实现），
//! **能力差异正是"新增数据源"对话框存在「能力」Tab 的理由**。但落进代码后能力有三个面，
//! 过去各说各话：
//!
//! | 面 | 过去的载体 | 问题 |
//! | --- | --- | --- |
//! | 界面 | `drivers.capabilities`（种子 JSON 数组）+ 视图里的 12 键标签表 | 键**没有任何门控**，纯粹展示；标签表是第二份字典 |
//! | 运行时 | `DataSourceMeta` 的布尔位（`traits.rs`） | 6 位里 5 位没有读者；且按**数据库族**给（同族两个实现共用一份） |
//! | 验收 | 文档表格 | 与代码无关联，容易"声明了就算支持"（本仓 D10：未真机验收不算可用） |
//!
//! 本模块把**键的语义**固定成一处：界面标签、运行时位映射、验收状态都从这里取；
//! 每个驱动**声明哪些键**仍来自 `drivers.capabilities`（驱动目录是读模型；
//! 决策 ② 落地后改由代码 upsert，届时键声明也进这里）。
//!
//! ## 不改什么
//!
//! 运行时的位仍然由**驱动自己**给出（`DataSourceMeta` 的各构造器）——驱动知道自己的实现细节；
//! 本字典只负责「键 ⇄ 位」的对应关系，并用测试盯住两边不漂移（`capability::tests`）。

use crate::driver::traits::DataSourceMeta;

/// 运行时能力位（`DataSourceMeta` 的布尔位）。
///
/// `None`（键没有对应位）= 纯界面能力（如 `table_editor`：是产品功能开关，不在驱动里判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaBit {
    Transaction,
    Streaming,
    Arrow,
    Federated,
    ConcurrentWrite,
    InMemory,
}

impl MetaBit {
    /// 该位在给定 meta 上的取值。
    pub fn of(self, meta: &DataSourceMeta) -> bool {
        match self {
            MetaBit::Transaction => meta.supports_transaction,
            MetaBit::Streaming => meta.supports_streaming,
            MetaBit::Arrow => meta.supports_arrow,
            MetaBit::Federated => meta.supports_federated,
            MetaBit::ConcurrentWrite => meta.supports_concurrent_write,
            MetaBit::InMemory => meta.is_in_memory,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            MetaBit::Transaction => "supports_transaction",
            MetaBit::Streaming => "supports_streaming",
            MetaBit::Arrow => "supports_arrow",
            MetaBit::Federated => "supports_federated",
            MetaBit::ConcurrentWrite => "supports_concurrent_write",
            MetaBit::InMemory => "is_in_memory",
        }
    }
}

/// 真机验收状态（D10 口径：未真机验收不算可用）。
///
/// `evidence` 写**可复现的用例名**（测试目标 / 用例），而不是"某天跑过"——
/// 界面据此如实标注，不假装通过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Acceptance {
    pub verified: bool,
    pub evidence: &'static str,
}

impl Acceptance {
    /// 已真机验收（附用例名）。
    pub const fn verified(evidence: &'static str) -> Self {
        Self {
            verified: true,
            evidence,
        }
    }

    /// 未真机验收（声明了但还没有真机证据）。
    pub const fn unverified() -> Self {
        Self {
            verified: false,
            evidence: "",
        }
    }

    /// 界面用的一句话（"已验收（用例）" / "未验收"）。
    pub fn label(&self) -> String {
        if self.verified {
            format!("已验收 · {}", self.evidence)
        } else {
            "未验收".to_string()
        }
    }
}

/// 一个能力键的定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilitySpec {
    /// 键（`drivers.capabilities` 里的字符串）。
    pub key: &'static str,
    /// 中文标签（界面唯一来源）。
    pub label: &'static str,
    /// 对应的运行时能力位；`None` = 纯界面能力。
    pub meta_bit: Option<MetaBit>,
    /// 真机验收状态。
    pub acceptance: Acceptance,
}

/// 能力字典（与种子 `drivers.capabilities` 的键集合一致；新增键请同时更新各驱动声明）。
pub const CAPABILITY_DICTIONARY: [CapabilitySpec; 12] = [
    CapabilitySpec {
        key: "tree",
        label: "数据库导航",
        meta_bit: None,
        // 导航树读元数据：真机覆盖面见 `db_navigator` / `editor_exec_real`（四库导航对象已跑通）
        acceptance: Acceptance::verified("db_navigator + editor_exec_real"),
    },
    CapabilitySpec {
        key: "health_check",
        label: "健康检查",
        meta_bit: None,
        // 连接探测：真机 192.168.3.138（MySQL/PG）+ 本地 SQLite/DuckDB 均有探测用例
        acceptance: Acceptance::verified("data_source_lifecycle + official_driver_real"),
    },
    CapabilitySpec {
        key: "transactions",
        label: "事务",
        meta_bit: Some(MetaBit::Transaction),
        acceptance: Acceptance::verified("editor_exec_real（四库事务：回滚作废/提交生效）"),
    },
    CapabilitySpec {
        key: "index_analysis",
        label: "索引分析",
        meta_bit: None,
        acceptance: Acceptance::unverified(),
    },
    CapabilitySpec {
        key: "sql_autocomplete",
        label: "SQL 补全",
        meta_bit: None,
        acceptance: Acceptance::unverified(),
    },
    CapabilitySpec {
        key: "schema_browser",
        label: "模式浏览",
        meta_bit: None,
        // PG/SQLite/DuckDB 的 schema 层：`insight_schema_real` 有真机覆盖
        acceptance: Acceptance::verified("insight_schema_real"),
    },
    CapabilitySpec {
        key: "table_editor",
        label: "表编辑器",
        meta_bit: None,
        acceptance: Acceptance::unverified(),
    },
    CapabilitySpec {
        key: "analytics",
        label: "分析查询",
        meta_bit: None,
        // 结果二次分析/本地加速：`editor_exec_real` 的加速通道（DuckDB 真机）
        acceptance: Acceptance::verified("editor_exec_real（本地加速通道）"),
    },
    CapabilitySpec {
        key: "federation",
        label: "联邦查询",
        meta_bit: Some(MetaBit::Federated),
        acceptance: Acceptance::verified("federation_sources（mysql_native 跨源）"),
    },
    CapabilitySpec {
        key: "export",
        label: "数据导出",
        meta_bit: None,
        acceptance: Acceptance::unverified(),
    },
    CapabilitySpec {
        key: "mock",
        label: "Mock 生成",
        meta_bit: None,
        acceptance: Acceptance::unverified(),
    },
    CapabilitySpec {
        key: "resource",
        label: "资源分析",
        meta_bit: None,
        acceptance: Acceptance::unverified(),
    },
];

/// 字典里**没有**对应界面的运行时位（有意如此，改动前先读理由）。
///
/// 与 [`CAPABILITY_DICTIONARY`] 的 `meta_bit` 合起来必须覆盖所有 [`MetaBit`]——
/// 由 `capability::tests::every_meta_bit_is_accounted_for` 盯住：新增一个位就必须在这里
/// 或字典里表态，不允许"悄悄多一个没人管的位"。
pub const META_BITS_WITHOUT_UI_KEY: [MetaBit; 4] = [
    // 流式：`Database` trait 上没有流式接口（机制未接入），先不摆声明
    MetaBit::Streaming,
    // Arrow：本仓的 Arrow 通路读的是**查询结果**的 batch，该位目前只用于插件通信声明
    MetaBit::Arrow,
    // 并发写：没有读者（写路径不做并发判定）
    MetaBit::ConcurrentWrite,
    // 内存库：没有键，靠连接串 `:memory:` 表达
    MetaBit::InMemory,
];

/// 按键取定义（未收录 → `None`，调用方原样展示键，不丢信息）。
pub fn spec(key: &str) -> Option<&'static CapabilitySpec> {
    CAPABILITY_DICTIONARY.iter().find(|s| s.key == key)
}

/// 按键取中文标签（未收录 → 原样返回键）。
pub fn label(key: &str) -> String {
    spec(key)
        .map(|s| s.label.to_string())
        .unwrap_or_else(|| key.to_string())
}

/// 按键取运行时能力位。
pub fn meta_bit(key: &str) -> Option<MetaBit> {
    spec(key).and_then(|s| s.meta_bit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_keys_and_labels_are_unique_and_non_empty() {
        let mut keys: Vec<&str> = CAPABILITY_DICTIONARY.iter().map(|s| s.key).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n, "能力键必须唯一");
        for s in CAPABILITY_DICTIONARY {
            assert!(!s.label.trim().is_empty(), "{} 缺中文标签", s.key);
        }
    }

    /// 每个能力位只能被一个键映射，且「字典 + 有意不映射清单」必须穷尽所有位。
    ///
    /// 这条是给未来的自己看的：新增一个 `MetaBit` 而不表态，测试就红。
    #[test]
    fn every_meta_bit_is_accounted_for() {
        let all = [
            MetaBit::Transaction,
            MetaBit::Streaming,
            MetaBit::Arrow,
            MetaBit::Federated,
            MetaBit::ConcurrentWrite,
            MetaBit::InMemory,
        ];

        for bit in all {
            let mapped_by: Vec<&str> = CAPABILITY_DICTIONARY
                .iter()
                .filter(|s| s.meta_bit == Some(bit))
                .map(|s| s.key)
                .collect();
            let explicitly_unmapped = META_BITS_WITHOUT_UI_KEY.contains(&bit);
            assert!(
                mapped_by.len() == 1 || explicitly_unmapped,
                "{} 必须恰好被一个能力键映射，或列入 META_BITS_WITHOUT_UI_KEY",
                bit.key()
            );
            if explicitly_unmapped {
                assert!(
                    mapped_by.is_empty(),
                    "{} 同时被映射又被声明为无界面键，二选一",
                    bit.key()
                );
            }
        }
    }

    #[test]
    fn bit_reads_the_right_field() {
        let duck = DataSourceMeta::duckdb();
        assert!(MetaBit::Federated.of(&duck));
        assert!(!MetaBit::Federated.of(&DataSourceMeta::mysql()));
        assert!(MetaBit::Transaction.of(&DataSourceMeta::sqlite()));
    }

    #[test]
    fn unknown_key_falls_back_to_the_raw_key() {
        assert_eq!(label("brand_new"), "brand_new");
        assert!(spec("brand_new").is_none());
        assert!(meta_bit("brand_new").is_none());
        assert_eq!(meta_bit("transactions"), Some(MetaBit::Transaction));
    }

    /// 验收标记的两种形态（界面按此如实标注，不假装通过）。
    #[test]
    fn acceptance_labels_are_honest() {
        assert_eq!(
            Acceptance::verified("some_test").label(),
            "已验收 · some_test"
        );
        assert_eq!(Acceptance::unverified().label(), "未验收");
        assert!(!Acceptance::unverified().verified);
    }

    /// 驱动 id → 运行时能力原型（与各驱动 `meta()` 里的 `..DataSourceMeta::xxx()` 同一份）。
    fn prototype(driver_id: &str) -> Option<DataSourceMeta> {
        Some(match driver_id {
            "mysql" => DataSourceMeta::mysql(),
            "mysql_native" => DataSourceMeta::mysql_native(),
            "postgres" => DataSourceMeta::postgres(),
            "postgres_native" => DataSourceMeta::postgres_native(),
            "sqlite" => DataSourceMeta::sqlite(),
            "duckdb" => DataSourceMeta::duckdb(),
            _ => return None,
        })
    }

    /// 种子 `drivers.capabilities`（迁移 008 / 013）声明了哪些键——**fixture 镜像**，
    /// 先用它盯住“库里的声明 ↔ 代码里的能力位”不漂移；
    /// 决策 ②（代码声明 + 启动 upsert）落地后，这里改成直接读声明本身。
    const SEED_CAPABILITIES: [(&str, &[&str]); 6] = [
        (
            "mysql",
            &[
                "tree",
                "health_check",
                "transactions",
                "index_analysis",
                "sql_autocomplete",
                "table_editor",
            ],
        ),
        (
            "mysql_native",
            &[
                "tree",
                "health_check",
                "transactions",
                "index_analysis",
                "sql_autocomplete",
                "table_editor",
            ],
        ),
        (
            "postgres",
            &[
                "tree",
                "health_check",
                "transactions",
                "index_analysis",
                "sql_autocomplete",
                "schema_browser",
                "table_editor",
            ],
        ),
        (
            "postgres_native",
            &[
                "tree",
                "health_check",
                "transactions",
                "index_analysis",
                "sql_autocomplete",
                "schema_browser",
                "table_editor",
            ],
        ),
        (
            "sqlite",
            &[
                "tree",
                "health_check",
                "transactions",
                "index_analysis",
                "sql_autocomplete",
                "table_editor",
            ],
        ),
        (
            "duckdb",
            &[
                "tree",
                "health_check",
                "transactions",
                "sql_autocomplete",
                "schema_browser",
                "analytics",
                "federation",
                "table_editor",
            ],
        ),
    ];

    /// 种子声明里的键必须在字典里（否则界面只能原样显示内部名）；
    /// 反向：字典里带键的位，驱动原型为 true 就必须在种子里声明。
    ///
    /// 这两个方向合起来才是“能力 Tab 的声明 ↔ 运行时的位”真正对上——
    /// 真机已经证明同族两个实现会不一样（TLS 能力），
    /// 所以不能拿“族”将就，必须逐驱动对。
    #[test]
    fn seed_declarations_and_runtime_bits_agree() {
        for (driver_id, declared) in SEED_CAPABILITIES {
            let meta = prototype(driver_id).unwrap_or_else(|| panic!("{driver_id} 缺原型"));

            // 方向一：声明了键 → 键认识，且若有运行时位则必须为 true
            for key in declared {
                let spec = spec(key).unwrap_or_else(|| panic!("{driver_id} 声明了字典外的键 {key}"));
                if let Some(bit) = spec.meta_bit {
                    assert!(
                        bit.of(&meta),
                        "{driver_id} 声明了 {}（{bit:?}），但运行时原型为 false——两边必须一致",
                        spec.key,
                        bit = bit.key()
                    );
                }
            }

            // 方向二：原型为 true 的位，若该位有对应的界面键，就必须被声明
            for bit in [
                MetaBit::Transaction,
                MetaBit::Streaming,
                MetaBit::Arrow,
                MetaBit::Federated,
                MetaBit::ConcurrentWrite,
                MetaBit::InMemory,
            ] {
                if !bit.of(&meta) || META_BITS_WITHOUT_UI_KEY.contains(&bit) {
                    continue;
                }
                let key = CAPABILITY_DICTIONARY
                    .iter()
                    .find(|s| s.meta_bit == Some(bit))
                    .map(|s| s.key)
                    .expect("有界面键的位必在字典里");
                assert!(
                    declared.contains(&key),
                    "{driver_id} 的运行时位 {bit_key} 为 true，但种子未声明能力键 {key}",
                    bit_key = bit.key()
                );
            }
        }
    }
}
