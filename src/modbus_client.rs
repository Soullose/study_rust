use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task::JoinHandle;
use tokio::time::{Duration, interval};
use tokio_modbus::prelude::*;

pub struct ModbusHandle {
    pub join: JoinHandle<Result<()>>,
    stop_flag: Arc<AtomicBool>,
}

impl ModbusHandle {
    pub fn stop(&self) {
        self.stop_flag.store(false, Ordering::SeqCst);
    }

    pub async fn stop_and_wait(self) -> Result<()> {
        self.stop();
        self.join.await.unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())))
    }
}

// 移除泛型 runner，使用 TCP 专用实现以避免类型推断复杂性
pub fn run_modbus_client_tcp(socket_addr: SocketAddr, slave_id: u8, read_interval_secs: u64) -> ModbusHandle {
    let running = Arc::new(AtomicBool::new(true));
    let stop_flag = running.clone();

    let join = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(read_interval_secs));
        println!("开始周期性读取，间隔: {} 秒", read_interval_secs);
        println!("按 Ctrl+C 停止读取");

        // Ctrl+C 也可以设置停止标志
        let r = running.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            println!("接收到停止信号，正在停止...");
            r.store(false, Ordering::SeqCst);
        });

        let mut read_count: usize = 0;
        let max_reconnect_attempts = 5usize;
        let reconnect_interval = Duration::from_secs(5);

        while running.load(Ordering::SeqCst) {
            interval.tick().await;

            // 建立连接（含重连逻辑）
            let mut attempts = 0usize;
            let mut ctx_res: anyhow::Result<tokio_modbus::client::Context> = tcp::connect_slave(socket_addr, Slave(slave_id))
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()));

            while ctx_res.is_err() && attempts < max_reconnect_attempts && running.load(Ordering::SeqCst) {
                attempts += 1;
                eprintln!("连接失败，正在尝试第 {} 次重连...", attempts);
                tokio::time::sleep(reconnect_interval).await;
                ctx_res = tcp::connect_slave(socket_addr, Slave(slave_id))
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()));
            }

            let mut ctx = match ctx_res {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("达到最大重连次数，跳过本次读取: {}", e);
                    continue;
                }
            };

            read_count += 1;
            println!("\n第 {} 次读取:", read_count);

            let start_address: u16 = 0;
            let register_count: u16 = 5;

            match ctx.read_holding_registers(start_address, register_count).await {
                Ok(registers) => {
                    println!("成功读取寄存器:");
                    for (i, value) in registers.iter().enumerate() {
                        println!("寄存器 {}: {:?}", start_address + i as u16, value);
                    }
                }
                Err(e) => {
                    eprintln!("读取寄存器失败: {}", e);
                }
            }

            // 明确 drop 连接以关闭 socket
            drop(ctx);
        }

        println!("读取循环已退出，连接已关闭");
        Ok(())
    });

    ModbusHandle { join, stop_flag }
}

// 保留测试入口
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn smoke_tcp_runner() {
        let _ = run_modbus_client_tcp(SocketAddr::from_str("127.0.0.1:502").unwrap(), 1, 1);
    }
}
