// src/main.rs
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::{Duration, interval};
use tokio_modbus::prelude::*;

async fn run_modbus_client() -> Result<()> {
    // 设置 Modbus 服务器地址和端口
    let socket_addr = "127.0.0.1:502".parse().unwrap();

    // 建立 TCP 连接
    let mut ctx = match tcp::connect(socket_addr).await {
        Ok(ctx) => {
            println!("成功连接到 Modbus 服务器: {}", socket_addr);
            ctx
        }
        Err(e) => {
            return Err(anyhow!("连接失败: {}", e));
        }
    };

    // 设置读取间隔（秒）
    let read_interval_secs = 1;
    let mut interval = interval(Duration::from_secs(read_interval_secs));

    // 用于控制循环的原子布尔值
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // 设置 Ctrl+C 信号处理
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        println!("接收到停止信号，正在停止...");
        r.store(false, Ordering::SeqCst);
    });

    println!("开始周期性读取，间隔: {} 秒", read_interval_secs);
    println!("按 Ctrl+C 停止读取");

    // 读取计数器
    let mut read_count = 0;

    // 主循环
    while running.load(Ordering::SeqCst) {
        interval.tick().await;

        // 读取保持寄存器
        let start_address = 0;
        let register_count = 5;

        read_count += 1;
        println!("\n第 {} 次读取:", read_count);

        match ctx
            .read_holding_registers(start_address, register_count)
            .await
        {
            Ok(registers) => {
                println!("成功读取寄存器:");
                for (i, value) in registers.iter().enumerate() {
                    println!("寄存器 {}: {:?}", start_address + i as u16, value);
                }
            }
            Err(e) => {
                eprintln!("读取寄存器失败: {}", e);
                // 可以选择重连或退出
                // 这里简单打印错误并继续
            }
        }
    }

    // 关闭连接
    drop(ctx);
    println!("连接已关闭");

    Ok(())
}

fn main() -> Result<()> {
    // 创建 tokio 运行时并执行异步函数
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_modbus_client())
}
