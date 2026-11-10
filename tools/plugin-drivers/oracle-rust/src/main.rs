mod driver;
mod meta;
mod types;

use rds_driver_sidecar::serve;
use tokio::io::{stdin, stdout};

#[tokio::main]
async fn main() {
    if let Err(err) = serve(driver::OracleDriver, stdin(), stdout()).await {
        eprintln!("[rds-oracle-sidecar] {err}");
        std::process::exit(1);
    }
}
