// 临时探针：验证 oracle-rs 0.1.7 首页游标与 fetch_more 的真实行为。
// 用法：cargo run --example probe --release（凭据从环境变量读，默认用本机验证库）
use oracle_rs::{Config, Connection};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = std::env::var("RDS_ORA_HOST").unwrap_or_else(|_| "192.168.3.138".into());
    let port: u16 = std::env::var("RDS_ORA_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1521);
    let service = std::env::var("RDS_ORA_SERVICE").unwrap_or_else(|_| "XEPDB1".into());
    let user = std::env::var("RDS_ORA_USER").unwrap_or_else(|_| "devuser".into());
    let password = std::env::var("RDS_ORA_PASSWORD").unwrap_or_else(|_| "Dev2026123".into());
    let sql = std::env::var("RDS_ORA_SQL")
        .unwrap_or_else(|_| "select level as n from dual connect by level <= 3000".into());

    let config = Config::new(host, port, service, user, password).connect_timeout(Duration::from_secs(10));
    let conn = Connection::connect_with_config(config).await?;
    let first = conn.execute(&sql, &[]).await?;
    println!(
        "first rows={} cursor_id={} has_more={} columns={} rows_affected={}",
        first.rows.len(),
        first.cursor_id,
        first.has_more_rows,
        first.columns.len(),
        first.rows_affected
    );

    if first.cursor_id != 0 {
        let columns = first.columns.clone();
        let mut total = first.rows.len();
        let mut cursor = first.cursor_id;
        for page in 1..=5 {
            match conn.fetch_more(cursor, &columns, 1000).await {
                Ok(more) => {
                    println!(
                        "page{} rows={} cursor_id={} has_more={}",
                        page,
                        more.rows.len(),
                        more.cursor_id,
                        more.has_more_rows
                    );
                    total += more.rows.len();
                    if more.rows.is_empty() {
                        break;
                    }
                    if more.cursor_id != 0 {
                        cursor = more.cursor_id;
                    }
                }
                Err(err) => {
                    println!("page{} error={err}", page);
                    break;
                }
            }
        }
        println!("total={total}");
    }
    Ok(())
}
