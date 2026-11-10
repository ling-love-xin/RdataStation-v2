//! RdataStation SQL Server 原生 Rust 驱动 sidecar 入口。

mod driver;
mod meta;
mod types;

use std::process::ExitCode;

use rds_driver_sidecar::serve;
use tokio::io::{stdin, stdout};

use crate::driver::MssqlDriver;

#[tokio::main]
async fn main() -> ExitCode {
    match serve(MssqlDriver, stdin(), stdout()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[rds-mssql-sidecar] {err}");
            ExitCode::FAILURE
        }
    }
}
