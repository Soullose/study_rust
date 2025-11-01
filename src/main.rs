// src/main.rs
mod device_manager;
use crate::device_manager::DeviceManager;
use anyhow::Result;
use std::net::SocketAddr;
use tokio::time::{Duration, sleep};
use tokio_modbus::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 创建设备管理器
    let manager = DeviceManager::new();

    // 示例设备地址
    let addr1: SocketAddr = "127.0.0.1:502".parse().unwrap();

    // 使用 DeviceManager 的便捷方法添加 Modbus 设备，避免手动闭包寿命问题
    manager.add_modbus_device(addr1, 1, 2).await?;
    println!("已添加设备 {} slave {}", addr1, 1);

    println!("运行 10 秒...");
    sleep(Duration::from_secs(10)).await;

    // println!("正在停止所有设备...");
    // manager.stop_all().await?;
    // println!("已停止所有设备");
    println!("正在停止指定设备...");
    manager.stop_device(addr1, 1).await;
    println!("已停止指定设备");
    Ok(())
}
