# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\manager.rs")
t = p.read_text(encoding="utf-8")

# 1) open()：Windows 单连接兼容（不建多连接池）
old = """        // 创建写入连接
        let write_conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 写入连接失败: {}",
                e
            )))
        })?;

        // 配置写入连接
        Self::configure_connection(&write_conn)?;

        // 创建读取连接池
        let read_pool = Self::create_read_pool(path, DEFAULT_READ_POOL_SIZE)?;

        // 创建后台维护连接
        let maintenance_conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 维护连接失败: {}",
                e
            )))
        })?;

        Self::configure_connection(&maintenance_conn)?;

        Ok(DuckDBManager {
            db_path: path.to_path_buf(),
            write_conn,
            read_pool,
            maintenance_conn,
            read_index: AtomicUsize::new(0),
        })"""
new = """        // 创建唯一连接（Windows 平台限制：duckdb-rs bundled 同进程对同一
        // DuckDB 文件仅允许一个连接句柄，多连接在 Windows 报文件被占用；
        // 因此读/维护均复用该连接，由 DuckDB 自身支持单连接读写）。
        let write_conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 写入连接失败: {}",
                e
            )))
        })?;

        // 配置写入连接
        Self::configure_connection(&write_conn)?;

        Ok(DuckDBManager {
            db_path: path.to_path_buf(),
            write_conn,
            // Windows：读连接池留空，read_conn() 回退到唯一连接
            read_pool: Vec::new(),
            maintenance_conn: None,
            read_index: AtomicUsize::new(0),
        })"""
assert old in t, "open block not found"
t = t.replace(old, new)

# 2) read_conn()：池空时回退写连接
old2 = """    pub fn read_conn(&self) -> &Connection {
        let idx = self.read_index.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
        &self.read_pool[idx]
    }"""
new2 = """    pub fn read_conn(&self) -> &Connection {
        if self.read_pool.is_empty() {
            // Windows 单连接模式：读写共用唯一连接
            &self.write_conn
        } else {
            let idx = self.read_index.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
            &self.read_pool[idx]
        }
    }"""
assert old2 in t, "read_conn not found"
t = t.replace(old2, new2)

# 3) maintenance_conn()：Option 兼容（None → 写连接）
old3 = """    pub fn maintenance_conn(&self) -> &Connection {
        &self.maintenance_conn
    }"""
new3 = """    pub fn maintenance_conn(&self) -> &Connection {
        match &self.maintenance_conn {
            Some(c) => c,
            None => &self.write_conn,
        }
    }"""
assert old3 in t, "maintenance_conn not found"
t = t.replace(old3, new3)

p.write_text(t, encoding="utf-8")
print("manager.rs windows single-conn mode")
