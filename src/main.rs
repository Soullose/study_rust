// src/main.rs
mod modbus_client;
use crate::modbus_client::run_modbus_client_tcp;
use anyhow::Result;
use std::net::SocketAddr;
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main() -> Result<()> {
    // 示例：连接到本地主机的 Modbus TCP（端口 502），Slave ID = 1，每 2 秒读取一次
    let addr: SocketAddr = "127.0.0.1:502".parse().unwrap();
    let handle = run_modbus_client_tcp(addr, 1, 2);

    println!("Modbus client started. Running for 10s...");
    // 运行一段时间后停止（示例中 10 秒）
    sleep(Duration::from_secs(10)).await;

    println!("Stopping Modbus client...");
    handle.stop_and_wait().await?;
    println!("Modbus client stopped.");

    Ok(())
}
