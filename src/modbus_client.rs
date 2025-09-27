use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::{Duration, interval};
use tokio_modbus::prelude::*;
pub async fn run_modbus_client(socket_addr: SocketAddr, slave_id: u8, read_interval_secs: u64) -> Result<()> {
    // 设置读取间隔（秒）
    let mut interval = interval(Duration::from_secs(read_interval_secs));

    // 用于控制循环的原子布尔值
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // 设置 Ctrl+C 信号处理
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        println!("接收到停止信号，正在停止...");
        r.store(false, Ordering::SeqCst);
    });

    println!("开始周期性读取，间隔: {} 秒", read_interval_secs);
    println!("按 Ctrl+C 停止读取");

    // 读取计数器
    let mut read_count = 0;

    // 重连配置
    let max_reconnect_attempts = 5; // 最大重连尝试次数
    let reconnect_interval = Duration::from_secs(5); // 重连间隔

    // 主循环
    while running.load(Ordering::SeqCst) {
        interval.tick().await;

        // 尝试连接或重连
        let mut ctx = match tcp::connect_slave(socket_addr, Slave(slave_id)).await {
            Ok(ctx) => {
                println!("成功连接到 Modbus 服务器: {}", socket_addr);
                ctx
            }
            Err(e) => {
                eprintln!("连接失败: {}, 尝试重连...", e);

                // 重连逻辑
                let mut attempts = 0;
                let mut connected = false;
                let mut new_ctx = None;

                while attempts < max_reconnect_attempts && running.load(Ordering::SeqCst) {
                    attempts += 1;
                    println!("第 {} 次重连尝试...", attempts);

                    tokio::time::sleep(reconnect_interval).await;

                    match tcp::connect_slave(socket_addr, Slave(slave_id)).await {
                        Ok(ctx) => {
                            println!("重连成功!");
                            new_ctx = Some(ctx);
                            connected = true;
                            break;
                        }
                        Err(e) => {
                            eprintln!("重连失败: {}", e);
                        }
                    }
                }

                if !connected {
                    eprintln!("达到最大重连次数，停止尝试");
                    break;
                }

                new_ctx.unwrap()
            }
        };

        read_count += 1;
        println!("\n第 {} 次读取:", read_count);

        // 读取保持寄存器
        let start_address = 0;
        let register_count = 5;

        match ctx.read_holding_registers(start_address, register_count).await {
            Ok(registers) => {
                println!("成功读取寄存器:");
                for (i, value) in registers.iter().enumerate() {
                    println!("寄存器 {}: {:?}", start_address + i as u16, value);
                }

                // 如果读取成功，继续使用当前连接进行下一次读取
                drop(ctx); // 显式释放连接，下次循环会重新建立
            }
            Err(e) => {
                eprintln!("读取寄存器失败: {}", e);
                // 读取失败，连接可能已断开，下次循环会尝试重连
                drop(ctx); // 释放当前连接
            }
        }
    }

    println!("连接已关闭");
    Ok(())
}
