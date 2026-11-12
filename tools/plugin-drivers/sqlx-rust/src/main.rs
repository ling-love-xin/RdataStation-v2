//! RdataStation MySQL / PostgreSQL **sqlx 轨**驱动 sidecar 入口。
//!
//! 一个进程供两个 driver id（`mysql` / `postgres`）：宿主协议本来就按
//! `session.open` 的 `driver_id` 路由、`initialize` 也回 `driver_ids` 列表，
//! 多驱动是同一条协议的自然形态（见 `rust-core/src/server.rs` 的 `serve_many`）。
//!
//! 与内核 `crates/driver/src/native/{mysql,postgres}.rs` 的关系：
//! * 语义（单连接、URL 拼装、SSL 归一、元数据 SQL、单元格→文本解码）对齐；
//! * 元数据 SQL 直接 path 依赖 `rds-driver-sql`，**不抄第二份**；
//! * 能力面按 **sidecar 协议真正能表达的** 如实声明，做不到的置灰而不是静默降级
//!   （索引/约束明细、DDL 原文、触发器、例程源码在当前协议里没有通道）。

mod config;
mod mysql;
mod pg;
mod values;

use std::process::ExitCode;

use rds_driver_sidecar::{as_sidecar, serve_many};
use tokio::io::{stdin, stdout};

use crate::mysql::MysqlDriver;
use crate::pg::PostgresDriver;

#[tokio::main]
async fn main() -> ExitCode {
    let drivers = vec![as_sidecar(MysqlDriver), as_sidecar(PostgresDriver)];
    match serve_many(drivers, stdin(), stdout()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[rds-sqlx-sidecar] {err}");
            ExitCode::FAILURE
        }
    }
}
