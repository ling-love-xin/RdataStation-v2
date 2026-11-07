// ==================== 场景模板解析 ====================

use super::*;

impl MockEngine {
    /// 解析场景模板里的**表间引用**，算出每条引用目标的取值域（生成前校验 + 采样依据）。
    ///
    /// 规则（全部可在生成前判定，不需要读任何已有数据）：
    ///
    /// 1. 父表必须在**同一个模板**里（跨模板引用不成立，那是别的项目/别的库的事了）；
    /// 2. 父列必须存在，且是 `AutoIncrement`——域 = `[start, start + step×(行数-1)]`；
    ///    非自增主键（uuid / 随机）算不出域，直接拒绝而不是去读已落地的数据。
    ///
    /// **不要求父表先于子表**：域由模板参数算出，与「哪张表先跑」无关——
    /// 引用自身（自关联）或引用后面才生成的表都是合法的，跑完整个场景两张表都在。
    ///
    /// 返回的域清单已按 `(父表, 父列)` 去重；面板也调它做**提交前**校验。
    pub fn resolve_reference_domains(
        template: &ScenarioTemplate,
    ) -> MockResult<Vec<ReferenceDomain>> {
        let mut domains: Vec<ReferenceDomain> = Vec::new();
        for table in &template.tables {
            for col in &table.columns {
                let Some(dep) = &col.dependency else { continue };
                let parent_name = dep.ref_table.as_str();
                let parent_column = dep.ref_column.as_str();
                if parent_name.is_empty() || parent_column.is_empty() {
                    return Err(MockError::InvalidColumn(format!(
                        "表 '{}' 的列 '{}' 声明了引用但没写清父表 / 父列",
                        table.name, col.name
                    )));
                }
                let Some(parent_idx) = template.tables.iter().position(|t| t.name == parent_name)
                else {
                    return Err(MockError::InvalidColumn(format!(
                        "表 '{}' 的列 '{}' 引用了模板里没有的表 '{parent_name}'",
                        table.name, col.name
                    )));
                };
                let parent = &template.tables[parent_idx];
                let Some(parent_col) = parent.columns.iter().find(|c| c.name == parent_column)
                else {
                    return Err(MockError::InvalidColumn(format!(
                        "表 '{}' 的列 '{}' 引用了 '{parent_name}.{parent_column}'，\
                         但父表里没有这一列",
                        table.name, col.name
                    )));
                };
                let GeneratorConfig::AutoIncrement { start, step } = parent_col.generator else {
                    return Err(MockError::InvalidColumn(format!(
                        "父表 '{parent_name}' 的列 '{parent_column}' 不是自增生成，算不出取值域——\
                         引用目标目前只支持自增主键（不读已生成的数据）"
                    )));
                };
                if parent.row_count == 0 {
                    return Err(MockError::InvalidColumn(format!(
                        "父表 '{parent_name}' 的行数为 0，没有可引用的主键值"
                    )));
                }
                if domains
                    .iter()
                    .any(|d| d.table == parent_name && d.column == parent_column)
                {
                    continue;
                }
                domains.push(ReferenceDomain {
                    table: parent_name.to_string(),
                    column: parent_column.to_string(),
                    first: i64::from(start),
                    step: i64::from(step),
                    count: parent.row_count,
                });
            }
        }
        Ok(domains)
    }

    /// 从域清单里找某条引用对应的父表取值域（不存在 → `None`，由调用方转成可读错误）。
    pub(crate) fn domain_for<'a>(
        dep: &ColumnDependency,
        domains: &'a [ReferenceDomain],
    ) -> Option<&'a ReferenceDomain> {
        let table = dep.ref_table.as_str();
        let column = dep.ref_column.as_str();
        domains
            .iter()
            .find(|d| d.table == table && d.column == column)
    }

    /// 多表场景批量生成
    ///
    /// 根据场景模板（ScenarioTemplate）一次性生成所有关联表。
    /// 每张表独立生成，支持进度回调和取消检查。
    /// 模板里声明了**表间引用**的列（列上带 `dependency`）从**父表主键域**取值，
    /// 域由父列的自增参数与行数算出——不读任何已落地的数据（见 [`Self::resolve_reference_domains`]）。
    /// 返回 `MockScenarioResult` 包含每张表的生成结果摘要。
    ///
    /// S4：`cancel` 是本任务的取消令牌，在**表边界**响应（一张表一个检查点）。
    pub async fn generate_scenario(
        template: &ScenarioTemplate,
        cancel: Arc<AtomicBool>,
        on_table_progress: impl Fn(usize, usize) + Send + 'static,
    ) -> MockResult<MockScenarioResult> {
        let start = Instant::now();
        let mut table_results = Vec::new();
        let mut total_rows = 0u32;
        // 一次生成共用一份域清单：父表先于子表，因此下游能直接查
        let domains = Self::resolve_reference_domains(template)?;

        for (idx, table) in template.tables.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(MockError::Generation("场景生成已取消".to_string()));
            }

            // S5：场景级种子 → 每表派生种子（`seed + 表下标`）——整组可复现，
            // 且表间不会因同种子产出相同数据（跨表引用域由参数算出，不受种子影响）。
            let table_seed = template.seed.map(|base| base.wrapping_add(idx as u32));
            let config = MockConfig {
                table_name: table.name.clone(),
                row_count: table.row_count,
                seed: table_seed,
                locale: match template.locale.as_str() {
                    "zh_cn" => Locale::ZhCn,
                    "en" | "en_us" => Locale::En,
                    "ja_jp" => Locale::JaJp,
                    _ => Locale::ZhCn,
                },
                columns: table.columns.clone(),
            };

            let result = Self::generate_table(config, &domains, cancel.clone(), |_, _| {}).await?;
            total_rows += result.row_count;

            table_results.push(MockScenarioTableResult {
                table_name: result.table_name.clone(),
                temp_table_name: result.temp_table_name,
                row_count: result.row_count,
                columns: result.columns,
                elapsed_ms: result.elapsed_ms,
            });

            on_table_progress(idx + 1, template.tables.len());
        }

        let total_elapsed_ms = start.elapsed().as_millis() as u32;

        Ok(MockScenarioResult {
            template_id: template.id.clone(),
            template_name: template.name.clone(),
            seed: template.seed,
            table_count: template.tables.len() as u32,
            total_rows,
            total_elapsed_ms,
            tables: table_results,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::generate_cell;
    use crate::models::Locale;
    use crate::models::{ColumnDataType, GeneratorConfig};
    use engine::CoreError;

    /// 日历字段全默认的 `DateTime`（测试里多数用例只关心区间）。
    pub(crate) fn plain_date_time(min: &str, max: &str) -> GeneratorConfig {
        GeneratorConfig::DateTime {
            min: min.to_string(),
            max: max.to_string(),
            workdays_only: false,
            work_hours_only: false,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        }
    }

    /// 日历字段全默认的 `Date`。
    pub(crate) fn plain_date(min: &str, max: &str) -> GeneratorConfig {
        GeneratorConfig::Date {
            min: min.to_string(),
            max: max.to_string(),
            workdays_only: false,
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        }
    }

    /// 只开「仅工作时段」的 `DateTimeBetween`（用给定窗口）。
    pub(crate) fn hour_window(min: &str, start: &str, end: &str) -> GeneratorConfig {
        GeneratorConfig::DateTimeBetween {
            start: min.to_string(),
            end: "2024-12-31T23:59:59Z".to_string(),
            workdays_only: false,
            work_hours_only: true,
            work_hour_start: start.to_string(),
            work_hour_end: end.to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        }
    }

    #[test]
    pub(crate) fn test_sanitize_table_name_alphanumeric() {
        assert_eq!(sanitize_table_name("hello_world"), "hello_world");
    }

    #[test]
    pub(crate) fn test_sanitize_table_name_with_spaces() {
        assert_eq!(sanitize_table_name("my table"), "my_table");
    }

    #[test]
    pub(crate) fn test_sanitize_table_name_special_chars() {
        assert_eq!(sanitize_table_name("user-data@2024"), "user_data_2024");
    }

    #[test]
    pub(crate) fn test_build_create_table_ddl_single_column() {
        let cols = vec![ColumnDef {
            name: "id".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: true,
            dependency: None,
        }];
        let ddl = MockEngine::build_create_table_ddl("users", &cols).expect("列名合法");
        assert_eq!(ddl, "CREATE TABLE \"users\" (id INT UNIQUE NOT NULL)");
    }

    #[test]
    pub(crate) fn test_build_create_table_ddl_multi_column() {
        let cols = vec![
            ColumnDef {
                name: "id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                nullable_ratio: 0.0,
                unique: true,
                dependency: None,
            },
            ColumnDef {
                name: "name".to_string(),
                data_type: ColumnDataType::Varchar { length: Some(100) },
                generator: GeneratorConfig::Name,
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        ];
        let ddl = MockEngine::build_create_table_ddl("users", &cols).expect("列名合法");
        assert_eq!(
            ddl,
            "CREATE TABLE \"users\" (id INT UNIQUE NOT NULL, name VARCHAR(100) NOT NULL)"
        );
    }

    /// 列名要按 `sanitize_identifier` 落到 DDL 上：插入值时的列清单是同一算法得出的。
    ///
    /// 不这样做的后果是实测过的：`Order Date` 会直接产出非法 SQL
    /// （`CREATE TABLE ... (Order Date VARCHAR …)`）——DuckDB 报 `Parser Error`，
    /// 而用户完全看不出是「列名带空格」造成的。
    #[test]
    pub(crate) fn create_table_ddl_sanitizes_column_names() {
        let cols = vec![
            ColumnDef {
                name: "Order Date".to_string(),
                data_type: ColumnDataType::Date,
                generator: plain_date("2024-01-01", "2024-12-31"),
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
            ColumnDef {
                name: "#序号".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        ];
        let ddl = MockEngine::build_create_table_ddl("probe_space", &cols).expect("能建表");
        assert!(ddl.contains("Order_Date"), "{ddl}");
        assert!(ddl.contains("序号"), "{ddl}");
        assert!(!ddl.contains("Order Date"), "{ddl}");
    }

    /// 净化后为空 / 重名要在建表前报可读错误，而不是把 DuckDB 的原始报错扔给用户。
    #[test]
    pub(crate) fn create_table_ddl_reports_unusable_column_names() {
        let blank = vec![ColumnDef {
            name: "***".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        }];
        let reason = MockEngine::build_create_table_ddl("probe_blank", &blank)
            .expect_err("全符号列名要拒")
            .to_string();
        assert!(reason.contains("去掉符号后为空"), "{reason}");

        let duplicated = vec![
            ColumnDef {
                name: "user id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
            ColumnDef {
                name: "user_id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        ];
        let reason = MockEngine::build_create_table_ddl("probe_dup", &duplicated)
            .expect_err("净化后重名要拒")
            .to_string();
        assert!(reason.contains("同名"), "{reason}");
    }

    #[test]
    pub(crate) fn test_generate_cell_auto_increment() {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::AutoIncrement {
            start: 100,
            step: 5,
        };
        let val = generate_cell(&generator, &mut rng, 3, &Locale::ZhCn);
        assert_eq!(val, "115");
    }

    #[test]
    pub(crate) fn test_generate_cell_random_int_range() -> Result<(), CoreError> {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::RandomInt { min: 10, max: 20 };
        let val: i64 = generate_cell(&generator, &mut rng, 0, &Locale::ZhCn)
            .parse()
            .map_err(|e| CoreError::from(format!("parse int error: {}", e)))?;
        assert!((10..=20).contains(&val));
        Ok(())
    }

    #[test]
    pub(crate) fn test_generate_cell_constant() {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::Constant {
            value: "hello".to_string(),
        };
        let val = generate_cell(&generator, &mut rng, 0, &Locale::ZhCn);
        assert_eq!(val, "hello");
    }

    #[test]
    pub(crate) fn test_generate_cell_boolean() {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::Boolean { ratio: 100 };
        let val = generate_cell(&generator, &mut rng, 0, &Locale::ZhCn);
        assert_eq!(val, "true");
    }

    /// 「不能采样」的参数必须在生成前被拦——它们原本会在工作线程里 panic，
    /// 而 panic 发生在持有内存库连接锁期间，会把整个进程的 mock 生成打坏（实测）。
    #[test]
    pub(crate) fn unique_column_with_narrow_domain_is_rejected_upfront() {
        let col = |generator: GeneratorConfig, nullable_ratio: f64| ColumnDef {
            name: "c".to_string(),
            data_type: ColumnDataType::Varchar { length: None },
            generator,
            nullable_ratio,
            unique: true,
            dependency: None,
        };

        // 取值集合只有 3 项，却要 1000 行唯一值：必须在生成前就说清楚
        let problem = unique_capacity_problem(
            &col(
                GeneratorConfig::ValueSet {
                    values: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                },
                0.0,
            ),
            1000,
        );
        let text = problem.expect("值域不足应报错");
        assert!(text.contains("至多产出 3 个不同取值"), "{text}");

        // Boolean 恒取一侧时只有一个取值
        assert!(
            unique_capacity_problem(&col(GeneratorConfig::Boolean { ratio: 0 }, 0.0), 100)
                .is_some()
        );
        // RandomInt 的域是 [min, max] 的整数个数
        assert!(
            unique_capacity_problem(
                &col(GeneratorConfig::RandomInt { min: 1, max: 10 }, 0.0),
                100
            )
            .is_some()
        );
        // 域正好够：不报
        assert!(
            unique_capacity_problem(
                &col(GeneratorConfig::RandomInt { min: 1, max: 10 }, 0.0),
                10
            )
            .is_none()
        );
        // 全空列的唯一性天然成立（多个 NULL 不违反唯一约束），不能误报
        assert!(
            unique_capacity_problem(
                &col(
                    GeneratorConfig::ValueSet {
                        values: vec!["a".to_string()],
                    },
                    1.0,
                ),
                1000,
            )
            .is_none()
        );
        // 取值域连续 / 极大的生成器不在这里误伤
        assert!(
            unique_capacity_problem(
                &col(
                    GeneratorConfig::RandomFloat {
                        min: 0.0,
                        max: 1.0,
                        precision: 2
                    },
                    0.0
                ),
                100_000
            )
            .is_none()
        );
        assert!(
            unique_capacity_problem(
                &col(GeneratorConfig::AutoIncrement { start: 1, step: 1 }, 0.0),
                100_000,
            )
            .is_none()
        );
    }

    #[test]
    pub(crate) fn generator_params_that_cannot_be_sampled_are_rejected() {
        // 反向区间：实测 `RandomInt { min: 10, max: 5 }` 直接 panic
        assert!(generator_param_problem(&GeneratorConfig::RandomInt { min: 10, max: 5 }).is_some());
        // 半开区间：`min == max` 就是空区间（`Sentence { min: 1, max: 1 }` 实测过）
        assert!(generator_param_problem(&GeneratorConfig::Sentence { min: 1, max: 1 }).is_some());
        assert!(
            generator_param_problem(&GeneratorConfig::RandomFloat {
                min: 1.0,
                max: 1.0,
                precision: 2,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::RandomDecimal {
                min: 0.0,
                max: 0.0,
                scale: 2,
            })
            .is_some()
        );
        assert!(generator_param_problem(&GeneratorConfig::Words { min: 3, max: 2 }).is_some());
        assert!(generator_param_problem(&GeneratorConfig::Password { min: 8, max: 8 }).is_some());
        // 权重含负数：总权重可能 ≤ 0，抽 `0.0..total` 就是空区间
        assert!(
            generator_param_problem(&GeneratorConfig::Weighted {
                choices: vec![("a".to_string(), 5.0), ("b".to_string(), -5.0)],
            })
            .is_some()
        );
        // 时间区间不足一分钟：fake 走 `(0..分钟差)`，差 ≤ 0 即空区间
        assert!(
            generator_param_problem(&plain_date_time(
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:30Z",
            ))
            .is_some()
        );
        assert!(generator_param_problem(&plain_date("2024-12-31", "2024-01-01")).is_some());
        // 工作时段窗口：解析不了 / 零长窗口都拦
        assert!(
            generator_param_problem(&hour_window("2024-01-01T00:00:00Z", "9点半", "18:00"))
                .is_some()
        );
        assert!(
            generator_param_problem(&hour_window("2024-01-01T00:00:00Z", "09:00", "09:00"))
                .is_some()
        );
        // 24:00 只能当结束时刻
        assert!(
            generator_param_problem(&hour_window("2024-01-01T00:00:00Z", "24:00", "18:00"))
                .is_some()
        );
        // 分布族：参数越界会让抽样退化成常量 / NaN（λ ≤ 0、α ≤ 0、p 越界）
        assert!(generator_param_problem(&GeneratorConfig::Poisson { lambda: 0.0 }).is_some());
        assert!(generator_param_problem(&GeneratorConfig::Poisson { lambda: -1.0 }).is_some());
        assert!(
            generator_param_problem(&GeneratorConfig::Exponential { lambda: f64::NAN }).is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Pareto {
                scale_value: 0.0,
                alpha: 2.0,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Pareto {
                scale_value: 1.0,
                alpha: 0.0,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Beta {
                alpha: 0.0,
                beta: 2.0,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Binomial {
                trials: 0,
                probability: 0.5,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Binomial {
                trials: 10,
                probability: 1.5,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::TimeSeries {
                start: 0.0,
                trend: f64::INFINITY,
                period: 24,
                amplitude: 1.0,
                noise: 1.0,
            })
            .is_some()
        );
        // 工作日历：掩码不是 7 位 0/1、全 0、日期串不规范、步长不是整天
        let calendar =
            |work_week: &str, skip: &[&str], work: &[&str]| GeneratorConfig::SequentialDate {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 86_400,
                workdays_only: true,
                work_week: work_week.to_string(),
                skip_dates: skip.iter().map(|d| (*d).to_string()).collect(),
                work_dates: work.iter().map(|d| (*d).to_string()).collect(),
            };
        assert!(generator_param_problem(&calendar("111100", &[], &[])).is_some());
        assert!(generator_param_problem(&calendar("1111a00", &[], &[])).is_some());
        assert!(generator_param_problem(&calendar("0000000", &[], &[])).is_some());
        // `2026-1-1` 能被 chrono 解析，但与生成侧的字节比较对不上，必须拦
        assert!(generator_param_problem(&calendar("1111100", &["2026-1-1"], &[])).is_some());
        assert!(generator_param_problem(&calendar("1111100", &[], &["2026-13-01"])).is_some());
        // 步长不是整天（半天 / 负数）在「工作日」刻度上没意义
        assert!(
            generator_param_problem(&GeneratorConfig::SequentialDate {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 43_200,
                workdays_only: true,
                work_week: "1111100".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            })
            .is_some()
        );
        // 日历列表超长（生成期预算）
        let too_many: Vec<String> = (0..400)
            .map(|i| format!("2026-01-{:02}", (i % 28) + 1))
            .collect();
        assert!(
            generator_param_problem(&GeneratorConfig::DateTimeBetween {
                start: "2024-01-01T00:00:00Z".to_string(),
                end: "2024-12-31T23:59:59Z".to_string(),
                workdays_only: true,
                work_hours_only: true,
                work_hour_start: "09:00".to_string(),
                work_hour_end: "18:00".to_string(),
                work_week: "1111100".to_string(),
                skip_dates: too_many,
                work_dates: Vec::new(),
            })
            .is_some()
        );
    }

    /// 合法参数不能被误拦：护栏过宽会让正常配置也生不出来。
    #[test]
    pub(crate) fn valid_generator_params_pass_the_guard() {
        for generator in [
            // `RandomInt` 是**闭**区间，min == max 合法
            GeneratorConfig::RandomInt { min: 5, max: 5 },
            GeneratorConfig::RandomInt { min: 1, max: 10 },
            GeneratorConfig::RandomFloat {
                min: 0.0,
                max: 1.0,
                precision: 2,
            },
            GeneratorConfig::Sentence { min: 1, max: 3 },
            GeneratorConfig::Weighted {
                choices: vec![("a".to_string(), 1.0), ("b".to_string(), 2.0)],
            },
            plain_date_time("2020-01-01T00:00:00Z", "2025-12-31T23:59:59Z"),
            // 同一天的日期区间合法（下界 00:00:00 ~ 上界 23:59:59）
            plain_date("2024-01-01", "2024-01-01"),
            // 解析不出来的时间串不由这里拦：生成器自己会回退到默认窗口
            plain_date_time("2024-01-01", "2024-12-31"),
            GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            // 分布族边界：λ / α / p 取到端点值都合法
            GeneratorConfig::Poisson { lambda: 0.5 },
            GeneratorConfig::Exponential { lambda: 3.0 },
            GeneratorConfig::Pareto {
                scale_value: 1.0,
                alpha: 1.5,
            },
            GeneratorConfig::Beta {
                alpha: 2.0,
                beta: 5.0,
            },
            GeneratorConfig::Binomial {
                trials: 10,
                probability: 0.0,
            },
            GeneratorConfig::Binomial {
                trials: 10,
                probability: 1.0,
            },
            // `period` 为 0 表示「不叠加周期项」，是合法输入
            GeneratorConfig::TimeSeries {
                start: 0.0,
                trend: 0.0,
                period: 0,
                amplitude: 0.0,
                noise: 0.0,
            },
            // 工作日历：合法掩码（含单休、自定义工作周）+ 合法日期列表
            GeneratorConfig::SequentialDate {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 86_400,
                workdays_only: true,
                work_week: "1111100".to_string(),
                skip_dates: vec!["2024-10-01".to_string()],
                work_dates: vec!["2024-10-12".to_string()],
            },
            // 单休（周一~周六）也合法
            GeneratorConfig::SequentialDateWithGaps {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 172_800,
                miss_probability: 0.1,
                workdays_only: true,
                work_week: "1111110".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            },
            // 没勾「仅工作日」时，日历字段里留着旧值不该拦住生成
            GeneratorConfig::DateTimeBetween {
                start: "2024-01-01T00:00:00Z".to_string(),
                end: "2024-12-31T23:59:59Z".to_string(),
                workdays_only: false,
                work_hours_only: false,
                work_hour_start: "9点半".to_string(),
                work_hour_end: "18:00".to_string(),
                work_week: "111100".to_string(),
                skip_dates: vec!["2026-1-1".to_string()],
                work_dates: Vec::new(),
            },
            // 工作时段窗口：正常窗口、夜间跨零点、24:00 当结束都合法
            hour_window("2024-01-01T00:00:00Z", "10:30", "12:00"),
            hour_window("2024-01-01T00:00:00Z", "22:00", "06:00"),
            hour_window("2024-01-01T00:00:00Z", "08:00", "24:00"),
            // 只勾「仅工作时段」不勾「仅工作日」也合法（两者互不依赖）
            hour_window("2024-01-01T00:00:00Z", "09:00", "17:00"),
            // 日期列也可以只要工作日（没有工作时段这两个字段）
            GeneratorConfig::Date {
                min: "2024-01-01".to_string(),
                max: "2024-12-31".to_string(),
                workdays_only: true,
                work_week: "1111100".to_string(),
                skip_dates: vec!["2024-10-01".to_string()],
                work_dates: Vec::new(),
            },
        ] {
            assert!(
                generator_param_problem(&generator).is_none(),
                "{generator:?}"
            );
        }
    }

    /// 时间 / 日期 / 十进制 / 大整数 / 二进制都要有字面量：
    /// 这几类曾经整片漏掉（`_ => "NULL"`），导出的 `.sql` 会静默丢数据。
    #[test]
    pub(crate) fn sql_literals_cover_every_mock_column_type() {
        use duckdb::types::{Decimal, TimeUnit, Value};

        assert_eq!(
            value_to_sql_literal(&Value::Timestamp(TimeUnit::Second, 1_704_164_645)),
            "TIMESTAMP '2024-01-02 03:04:05'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Date32(19_724)),
            "DATE '2024-01-02'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Time64(TimeUnit::Second, 5)),
            "TIME '00:00:05'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Decimal(Decimal::new(4, 2, 1234).expect("decimal"))),
            "12.34"
        );
        assert_eq!(
            value_to_sql_literal(&Value::HugeInt(i128::from(i64::MAX) + 1)),
            "9223372036854775808"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Blob(vec![0x41, 0x0A])),
            "'\\x41\\x0A'::BLOB"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Text("it's".to_string())),
            "'it''s'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Interval {
                months: 1,
                days: 0,
                nanos: 3_600_000_000_000,
            }),
            "INTERVAL '1 months 01:00:00'"
        );
    }
}
