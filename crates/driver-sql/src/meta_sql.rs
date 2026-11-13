//! 元数据面 SQL 常量（同一族的多条实现共用一份，**勿抄第二份**）。
//!
//! 这里只有字符串：参数占位符也按各客户端库的原样保留（PG 用 `$1`，MySQL 用 `?`），
//! 因为「谁去绑参数」是驱动实现的事，不是 SQL 文本的事。
//!
//! ## 列序契约
//!
//! 三个 `*_TABLE_DETAIL_SQL`（PG / MySQL / DuckDB）输出**同一列序**，
//! 由内核的 `columns_from_detail_rows` 按位取值：
//!
//! `0 列名 / 1 类型 / 2 可空(YES|NO) / 3 序号(1 基) / 4 主键标记(PRI|空) / 5 默认值 /
//! 6 注释 / 7 引用表 / 8 引用列 / 9 外键标记(1|空，**含复合外键**) / 10 标识标记(1|空) /
//! 11 标识生成方式 / 12 排序规则`
//!
//! 10..=12 为 2026-10-24 列字段增强新增（属性面板「标识 / 排序规则」列）：
//! PG 用 `attidentity` + `attcollation`（`pg_collation.collname`），MySQL 用
//! `EXTRA` 含 `auto_increment` + `COLLATION_NAME`；DuckDB 无这两个概念，用空
//! 串占位**保持列序契约对齐**（映射按位取值，缺列会错位）。

/// PostgreSQL：列出某表的列（两个 PG 驱动共用）。
///
/// 列序契约见本模块头部说明。
///
/// **PG 的这三条不吃 `catalog` 参数**：一条连接只绑一个库，内省只能在当前库里做
/// （跨库浏览要靠「按库另开连接」，见内核 `get_catalogs` 的说明）。
///
/// 一次取齐属性面板要的东西：列名 / 类型 / 可空 / 主键标记 / 默认值 / 注释，
/// 外加 2026-09-21 补的 **`ordinal_position`** 与**单列外键的引用目标**
/// （`referential_constraints` 只含外键，所以不会把主键误认成引用；
/// 复合外键用 `COUNT(*) = 1` 挡掉 —— 那时的配对要按列序做，留给约束分区）。
pub const PG_TABLE_DETAIL_SQL: &str = "\
            SELECT a.attname, \
                   format_type(a.atttypid, a.atttypmod), \
                   CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END, \
                   a.attnum::text AS attnum, \
                   CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i \
                                      WHERE i.indrelid = c.oid AND i.indisprimary \
                                        AND a.attnum = ANY (i.indkey)) \
                        THEN 'PRI' ELSE '' END AS column_key, \
                   pg_catalog.pg_get_expr(ad.adbin, ad.adrelid), \
                   COALESCE(pg_catalog.col_description(c.oid, a.attnum), ''), \
                   COALESCE((SELECT rc.relname \
                               FROM pg_catalog.pg_constraint con \
                               JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid \
                              WHERE con.conrelid = c.oid AND con.contype = 'f' \
                                AND array_length(con.conkey, 1) = 1 \
                                AND con.conkey[1] = a.attnum \
                              LIMIT 1), '') AS ref_table, \
                   COALESCE((SELECT ra.attname \
                               FROM pg_catalog.pg_constraint con \
                               JOIN pg_catalog.pg_attribute ra \
                                 ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[1] \
                              WHERE con.conrelid = c.oid AND con.contype = 'f' \
                                AND array_length(con.conkey, 1) = 1 \
                                AND con.conkey[1] = a.attnum \
                              LIMIT 1), '') AS ref_column, \
                   CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_constraint con \
                                      WHERE con.conrelid = c.oid AND con.contype = 'f' \
                                        AND a.attnum = ANY (con.conkey)) \
                        THEN '1' ELSE '' END AS is_fk, \
                   CASE WHEN a.attidentity <> '' THEN '1' ELSE '' END AS is_identity, \
                   CASE a.attidentity WHEN 'd' THEN 'BY DEFAULT' WHEN 'a' THEN 'ALWAYS' \
                        ELSE '' END AS identity_generation, \
                   CASE WHEN a.attcollation <> 0::oid THEN \
                        (SELECT coll.collname FROM pg_catalog.pg_collation coll \
                          WHERE coll.oid = a.attcollation) ELSE '' END AS collation \
              FROM pg_catalog.pg_class c \
              JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
              JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid \
              LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid = c.oid AND ad.adnum = a.attnum \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum";

/// PostgreSQL：列出**服务器上可连接的库**（两个 PG 驱动共用）。
///
/// 为什么不是 `current_database()`：那样导航里只看得到一个库，用户以为别的库不见了
/// （实测端点上就有两个库）。别的库里有什么要**另开一条连到那个库的连接**才看得到 ——
/// 见 `MetadataBrowser::catalog_is_connection_scoped` 与
/// `ConnectionManager::get_scoped_connection`。
///
/// `datallowconn` 挡掉不允许连接的库，`datistemplate` 挡掉模板库（`template0` / `template1`）——
/// 它们连上去也没有用户对象。
pub const PG_LIST_CATALOGS_SQL: &str = "\
            SELECT datname FROM pg_catalog.pg_database \
             WHERE datallowconn AND NOT datistemplate \
             ORDER BY datname";

/// PostgreSQL：这条连接绑的库（跨库判断用；结果由 `ConnectionManager` 按连接缓存，只查一次）。
pub const PG_CURRENT_DATABASE_SQL: &str = "SELECT current_database()::text AS datname";

/// PostgreSQL：列出用户 schema（两个 PG 驱动共用）。
///
/// 为什么不用 `information_schema.schemata`：它把 `pg_toast*` / `pg_temp_*` 一起列出来
/// （真机实测：28 个 schema 里 26 个是这类噪音，用户只关心剩下的 1-2 个），而且它是
/// 几十路 JOIN 的视图。`pg_` 前缀是 PG 保留给系统的（`pg_catalog` / `pg_toast` / `pg_temp_N`），
/// 按业界惯例（DBeaver 同口径）一律不显示。
pub const PG_LIST_SCHEMAS_SQL: &str = "\
            SELECT n.nspname FROM pg_catalog.pg_namespace n \
             WHERE n.nspname <> 'information_schema' \
               AND n.nspname NOT LIKE 'pg\\_%' \
               AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') \
             ORDER BY n.nspname";

/// PostgreSQL：列出某 schema 下的对象（两个 PG 驱动共用）。
///
/// 为什么不用 `information_schema.tables`：**物化视图（`relkind = 'm'`）与外部表（`'f'`）
/// 根本不在它里面**（真机实测：物化视图在导航里完全看不到），它也不含分区表（`'p'`）。
/// 走 `pg_class` 一次拿全，顺带快得多（没有那几十路 JOIN）。
///
/// `relkind` → 展示类型的映射：`r` 普通表 / `p` 分区表 / `f` 外部表 → `BASE TABLE`；
/// `v` 视图与 `m` **物化视图**都 → `VIEW`（我们的导航把物化视图并入「视图」，见
/// `docs/architecture/references/README.md` 的对照口径）。
///
/// 第 3/4 列（S3-② 分区/继承折叠）：`parent_name` = `pg_inherits` 的父表（单继承，
/// 一个子表至多一条记录，JOIN 不放大行数）；`is_partition` = `c.relispartition`
/// （PG 10+，小于该版本的本列恒为 `false`，折叠退化为「不区分分区/继承」）。
pub const PG_LIST_TABLES_SQL: &str = "\
            SELECT c.relname, \
                   CASE c.relkind \
                        WHEN 'v' THEN 'VIEW' \
                        WHEN 'm' THEN 'VIEW' \
                        ELSE 'BASE TABLE' END AS table_type, \
                   p.relname AS parent_name, \
                   c.relispartition AS is_partition \
              FROM pg_catalog.pg_class c \
              JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
              LEFT JOIN pg_catalog.pg_inherits i ON i.inhrelid = c.oid \
              LEFT JOIN pg_catalog.pg_class p ON p.oid = i.inhparent \
             WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'v', 'm', 'f') \
             ORDER BY c.relname";

/// MySQL：列出某表的列（两个 MySQL 驱动共用）。
///
/// 列序契约见本模块头部说明。
///
/// 与 PG 的差别：`key_column_usage` 自己就有 `referenced_table_name` /
/// `referenced_column_name`，不必绕 `referential_constraints` 那圈 JOIN；
/// 复合外键同样用「本约束只涉及一列」挡掉 —— **与 PG 同口径**，所以四个网络驱动
/// 交出来的 `references` 语义一致。
pub const MY_TABLE_DETAIL_SQL: &str = "\
            SELECT c.column_name, c.data_type, c.is_nullable, CAST(c.ordinal_position AS CHAR) AS ordinal_position, c.column_key, \
                   c.column_default, c.column_comment, \
                   COALESCE((SELECT kcu.referenced_table_name \
                               FROM information_schema.key_column_usage kcu \
                              WHERE kcu.table_schema = c.table_schema AND kcu.table_name = c.table_name \
                                AND kcu.column_name = c.column_name AND kcu.referenced_table_name IS NOT NULL \
                                AND (SELECT COUNT(*) FROM information_schema.key_column_usage k2 \
                                      WHERE k2.constraint_schema = kcu.constraint_schema \
                                        AND k2.constraint_name = kcu.constraint_name) = 1 \
                              LIMIT 1), '') AS ref_table, \
                   COALESCE((SELECT kcu.referenced_column_name \
                               FROM information_schema.key_column_usage kcu \
                              WHERE kcu.table_schema = c.table_schema AND kcu.table_name = c.table_name \
                                AND kcu.column_name = c.column_name AND kcu.referenced_column_name IS NOT NULL \
                                AND (SELECT COUNT(*) FROM information_schema.key_column_usage k2 \
                                      WHERE k2.constraint_schema = kcu.constraint_schema \
                                        AND k2.constraint_name = kcu.constraint_name) = 1 \
                              LIMIT 1), '') AS ref_column, \
                   CASE WHEN EXISTS (SELECT 1 FROM information_schema.key_column_usage kcu \
                                      WHERE kcu.table_schema = c.table_schema \
                                        AND kcu.table_name = c.table_name \
                                        AND kcu.column_name = c.column_name \
                                        AND kcu.referenced_table_name IS NOT NULL) \
                        THEN '1' ELSE '' END AS is_fk, \
                   CASE WHEN c.extra LIKE '%auto_increment%' THEN '1' ELSE '' END AS is_identity, \
                   CASE WHEN c.extra LIKE '%auto_increment%' THEN 'AUTO_INCREMENT' \
                        ELSE '' END AS identity_generation, \
                   COALESCE(c.collation_name, '') AS collation \
              FROM information_schema.columns c \
             WHERE c.table_schema = ? AND c.table_name = ? \
             ORDER BY c.ordinal_position";

/// DuckDB：列出某表的列。
///
/// 列序契约见本模块头部说明。三处 DuckDB 专有（都是真机探针问出来的）：
///
/// * 注释列在 `information_schema.columns` 里叫 **`COLUMN_COMMENT`**（不叫 `comment`，
///   写 `c.comment` 直接是 `Binder Error`）；
/// * 主键只存在于 `duckdb_constraints()`，且列名列是**列表**，所以用 `list_contains` 判在不在；
/// * 外键目标列也是列表，必须 `array_to_string` 化开（`row.get::<String>` 对列表列报
///   `Invalid column type List`）。
///
/// 复合外键用 `len(...) = 1` 挡掉（同 PG / MySQL 口径）。
pub const DUCK_TABLE_DETAIL_SQL: &str = "\
            SELECT c.column_name, c.data_type, c.is_nullable, CAST(c.ordinal_position AS VARCHAR) AS ordinal_position, \
                   CASE WHEN EXISTS (SELECT 1 FROM duckdb_constraints() k \
                                      WHERE k.table_name = c.table_name \
                                        AND k.constraint_type = 'PRIMARY KEY' \
                                        AND list_contains(k.constraint_column_names, c.column_name)) \
                        THEN 'PRI' ELSE '' END AS column_key, \
                   c.column_default, COALESCE(c.column_comment, '') AS column_comment, \
                   COALESCE((SELECT k.referenced_table FROM duckdb_constraints() k \
                              WHERE k.table_name = c.table_name AND k.constraint_type = 'FOREIGN KEY' \
                                AND list_contains(k.constraint_column_names, c.column_name) \
                                AND len(k.constraint_column_names) = 1 LIMIT 1), '') AS ref_table, \
                   COALESCE((SELECT array_to_string(k.referenced_column_names, ',') \
                               FROM duckdb_constraints() k \
                              WHERE k.table_name = c.table_name AND k.constraint_type = 'FOREIGN KEY' \
                                AND list_contains(k.constraint_column_names, c.column_name) \
                                AND len(k.constraint_column_names) = 1 LIMIT 1), '') AS ref_column, \
                   CASE WHEN EXISTS (SELECT 1 FROM duckdb_constraints() k \
                                      WHERE k.table_name = c.table_name \
                                        AND k.constraint_type = 'FOREIGN KEY' \
                                        AND list_contains(k.constraint_column_names, c.column_name)) \
                        THEN '1' ELSE '' END AS is_fk, \
                   '' AS is_identity, '' AS identity_generation, '' AS collation \
              FROM information_schema.columns c \
             WHERE c.table_schema = 'main' AND c.table_name = ? \
             ORDER BY c.ordinal_position";

/// MySQL：列出某表的约束（两个 MySQL 驱动共用，勿抄第二份）。
///
/// 三张 `information_schema` 表各管一段：`TABLE_CONSTRAINTS`（有哪些约束）→
/// `KEY_COLUMN_USAGE`（涉及哪些列、引用谁）→ `REFERENTIAL_CONSTRAINTS`（更新 / 删除
/// 规则）。`GROUP_CONCAT(... ORDER BY ORDINAL_POSITION)` 把组合约束的列按**声明顺序**
/// 拼回一个字符串 —— 组合主键的顺序有意义，不能靠聚合的默认顺序。
pub const MY_LIST_CONSTRAINTS_SQL: &str = "\
            SELECT tc.CONSTRAINT_NAME AS cname, tc.CONSTRAINT_TYPE AS ctype, \
                   COALESCE(GROUP_CONCAT(kcu.COLUMN_NAME ORDER BY kcu.ORDINAL_POSITION), '') AS cols, \
                   COALESCE(kcu.REFERENCED_TABLE_NAME, '') AS ref_table, \
                   COALESCE(GROUP_CONCAT(kcu.REFERENCED_COLUMN_NAME ORDER BY kcu.ORDINAL_POSITION), '') AS ref_cols, \
                   COALESCE(rc.UPDATE_RULE, '') AS upd, COALESCE(rc.DELETE_RULE, '') AS del \
              FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
              LEFT JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu \
                ON kcu.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
               AND kcu.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
               AND kcu.TABLE_SCHEMA = tc.TABLE_SCHEMA AND kcu.TABLE_NAME = tc.TABLE_NAME \
              LEFT JOIN INFORMATION_SCHEMA.REFERENTIAL_CONSTRAINTS rc \
                ON rc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
               AND rc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
             WHERE tc.TABLE_SCHEMA = ? AND tc.TABLE_NAME = ? \
             GROUP BY tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE, kcu.REFERENCED_TABLE_NAME, \
                      rc.UPDATE_RULE, rc.DELETE_RULE \
             ORDER BY tc.CONSTRAINT_NAME";

/// PostgreSQL：列出某表的索引（两个 PG 驱动共用，勿抄第二份）。
///
/// 用 `pg_catalog` 而不是 `information_schema`：后者没有 `indisprimary` 与索引方法
/// （btree / hash / gin / gist / brin）。布尔列在 SQL 里 `::text` —— 批读路径按
/// `StringArray` 取，混一个 BooleanArray 会让整列 downcast 落空。
pub const PG_LIST_INDEXES_SQL: &str = "\
            SELECT i.relname AS index_name, \
                   ix.indisunique::text, ix.indisprimary::text, \
                   COALESCE(am.amname, '') AS method, \
                   COALESCE((SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                             FROM unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) \
                             JOIN pg_catalog.pg_attribute a \
                               ON a.attrelid = ix.indrelid AND a.attnum = k.attnum), '') AS cols \
              FROM pg_catalog.pg_index ix \
              JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid \
              JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid \
              JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace \
              LEFT JOIN pg_catalog.pg_am am ON am.oid = i.relam \
             WHERE n.nspname = $1 AND t.relname = $2 \
             ORDER BY i.relname";

/// PostgreSQL：列出某表的约束（两个 PG 驱动共用）。
///
/// `conkey` / `confkey` 是 `int2[]`，用 `unnest ... WITH ORDINALITY` 按**列序**拼回名字
/// （组合主键的顺序有意义，不能靠 `string_agg` 的默认顺序）。`confupdtype` /
/// `confdeltype` 的 `' '`（空格）表示没写 ON UPDATE/DELETE，由 `pg_fk_action` 归 None。
pub const PG_LIST_CONSTRAINTS_SQL: &str = "\
            SELECT c.conname, c.contype::text, \
                   COALESCE((SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                             FROM unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) \
                             JOIN pg_catalog.pg_attribute a \
                               ON a.attrelid = c.conrelid AND a.attnum = k.attnum), '') AS cols, \
                   COALESCE(rc.relname, '') AS ref_table, \
                   COALESCE((SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                             FROM unnest(c.confkey) WITH ORDINALITY AS k(attnum, ord) \
                             JOIN pg_catalog.pg_attribute a \
                               ON a.attrelid = c.confrelid AND a.attnum = k.attnum), '') AS ref_cols, \
                   c.confupdtype::text, c.confdeltype::text \
              FROM pg_catalog.pg_constraint c \
              JOIN pg_catalog.pg_class t ON t.oid = c.conrelid \
              JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace \
              LEFT JOIN pg_catalog.pg_class rc ON rc.oid = c.confrelid \
             WHERE n.nspname = $1 AND t.relname = $2 \
             ORDER BY c.conname";
