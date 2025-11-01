#![allow(dead_code)]
#![allow(unused_imports)]

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, interval};
use tokio_modbus::prelude::*;

/// 设备运行句柄（轻量）
pub struct DeviceHandle {
    pub join: JoinHandle<Result<()>>,
    stop_flag: Arc<AtomicBool>,
}

impl DeviceHandle {
    /// 请求停止（非阻塞）
    pub fn stop(&self) {
        self.stop_flag.store(false, Ordering::SeqCst);
    }

    /// 请求停止并等待任务结束（消费句柄）
    pub async fn stop_and_wait(self) -> Result<()> {
        self.stop();
        self.join.await.unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())))
    }
}

/// 通用设备工作器（零成本抽象）
///
/// - `C`：连接上下文类型（例如 tokio_modbus::client::Context）
/// - `FConnect`：建立连接的函数/闭包, async fn(SocketAddr, u8) -> anyhow::Result<C>
/// - `FRead`：读取数据的函数/闭包, async fn(&mut C, u16, u16) -> anyhow::Result<Vec<u16>>
///
/// 通过泛型和泛型闭包单态化实现零成本抽象（无动态分发）。
pub fn start_device_worker<C, FConnect, FutConnect, FRead, FutRead>(
    connect_fn: FConnect,
    read_fn: FRead,
    socket_addr: SocketAddr,
    slave_id: u8,
    read_interval_secs: u64,
) -> DeviceHandle
where
    C: Send + 'static,
    FConnect: Fn(SocketAddr, u8) -> FutConnect + Send + Sync + 'static,
    FutConnect: std::future::Future<Output = Result<C>> + Send,
    FRead: Fn(&mut C, u16, u16) -> FutRead + Send + Sync + 'static,
    FutRead: std::future::Future<Output = Result<Vec<u16>>> + Send,
{
    let running = Arc::new(AtomicBool::new(true));
    let stop_flag = running.clone();

    let join = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(read_interval_secs));
        println!("设备工作器启动，间隔: {} 秒", read_interval_secs);

        // 通过 Ctrl+C 也能停止
        let r = running.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            println!("接收到停止信号（device worker）");
            r.store(false, Ordering::SeqCst);
        });

        let mut read_count: usize = 0;
        let max_reconnect_attempts = 5usize;
        let reconnect_interval = Duration::from_secs(5);

        while running.load(Ordering::SeqCst) {
            interval.tick().await;

            // 建立连接并支持简单重连
            let mut attempts = 0usize;
            let mut conn_res = connect_fn(socket_addr, slave_id).await;

            while conn_res.is_err() && attempts < max_reconnect_attempts && running.load(Ordering::SeqCst) {
                attempts += 1;
                eprintln!("连接失败，重试第 {} 次...", attempts);
                tokio::time::sleep(reconnect_interval).await;
                conn_res = connect_fn(socket_addr, slave_id).await;
            }

            let mut conn = match conn_res {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("重连失败，跳过本次读取: {}", e);
                    continue;
                }
            };

            read_count += 1;
            println!("\n设备第 {} 次读取:", read_count);

            let start_address: u16 = 0;
            let register_count: u16 = 5;

            match read_fn(&mut conn, start_address, register_count).await {
                Ok(registers) => {
                    println!("读取成功: {:?}", registers);
                }
                Err(e) => {
                    eprintln!("读取失败: {}", e);
                }
            }

            // 明确释放连接
            drop(conn);
        }

        println!("设备工作器退出");
        Ok(())
    });

    DeviceHandle { join, stop_flag }
}

/// 设备管理器：管理多个设备对应的独立 worker（每个设备一个任务/句柄）
pub struct DeviceManager {
    inner: Mutex<HashMap<(SocketAddr, u8), DeviceHandle>>,
}

impl DeviceManager {
    /// 创建一个空的 DeviceManager
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 添加设备并启动对应的 worker（如果已存在则返回已有的句柄）
    /// 使用泛型参数允许零成本抽象：connect_fn 和 read_fn 会在调用处被单态化。
    pub async fn add_device<C, FConnect, FutConnect, FRead, FutRead>(
        &self,
        connect_fn: FConnect,
        read_fn: FRead,
        socket_addr: SocketAddr,
        slave_id: u8,
        read_interval_secs: u64,
    ) -> Result<()>
    where
        C: Send + 'static,
        FConnect: Fn(SocketAddr, u8) -> FutConnect + Send + Sync + 'static,
        FutConnect: std::future::Future<Output = Result<C>> + Send,
        FRead: Fn(&mut C, u16, u16) -> FutRead + Send + Sync + 'static,
        FutRead: std::future::Future<Output = Result<Vec<u16>>> + Send,
    {
        let key = (socket_addr, slave_id);
        let mut map = self.inner.lock().await;
        if map.contains_key(&key) {
            // already exists
            return Ok(());
        }

        let handle = start_device_worker(connect_fn, read_fn, socket_addr, slave_id, read_interval_secs);
        map.insert(key, handle);
        Ok(())
    }

    /// 便捷方法：添加一个基于 tokio-modbus 的 Modbus TCP 设备
    pub async fn add_modbus_device(&self, socket_addr: SocketAddr, slave_id: u8, read_interval_secs: u64) -> Result<()> {
        let key = (socket_addr, slave_id);
        let mut map = self.inner.lock().await;
        if map.contains_key(&key) {
            return Ok(());
        }

        // 为该设备创建独立任务（专用实现，避免闭包生命周期问题）
        let running = Arc::new(AtomicBool::new(true));
        let stop_flag = running.clone();

        let addr = socket_addr;
        let sid = slave_id;
        let interval_secs = read_interval_secs;

        let join = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_secs));
            println!("Modbus 设备 worker 启动: {} slave {}，间隔 {}s", addr, sid, interval_secs);

            let r = running.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                println!("接收到停止信号（modbus device）");
                r.store(false, Ordering::SeqCst);
            });

            let mut read_count: usize = 0;
            let max_reconnect_attempts = 5usize;
            let reconnect_interval = Duration::from_secs(5);

            while running.load(Ordering::SeqCst) {
                interval.tick().await;

                let mut attempts = 0usize;
                let mut ctx_res = tcp::connect_slave(addr, Slave(sid))
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()));

                while ctx_res.is_err() && attempts < max_reconnect_attempts && running.load(Ordering::SeqCst) {
                    attempts += 1;
                    eprintln!("连接失败，重试第 {} 次...", attempts);
                    tokio::time::sleep(reconnect_interval).await;
                    ctx_res = tcp::connect_slave(addr, Slave(sid))
                        .await
                        .map_err(|e| anyhow::anyhow!(e.to_string()));
                }

                let mut ctx = match ctx_res {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("重连失败，跳过本次读取: {}", e);
                        continue;
                    }
                };

                read_count += 1;
                println!("\n设备第 {} 次读取:", read_count);

                let start_address: u16 = 0;
                let register_count: u16 = 5;

                // 读取并扁平化可能的嵌套 Result
                match ctx.read_holding_registers(start_address, register_count).await {
                    Ok(inner) => match inner {
                        Ok(registers) => {
                            println!("读取成功: {:?}", registers);
                        }
                        Err(ex) => {
                            eprintln!("Modbus 从站异常: {}", ex);
                        }
                    },
                    Err(e) => {
                        eprintln!("读取失败: {}", e);
                    }
                }

                drop(ctx);
            }

            println!("Modbus 设备 worker 退出: {} slave {}", addr, sid);
            Ok(())
        });

        let handle = DeviceHandle { join, stop_flag };
        map.insert(key, handle);
        Ok(())
    }

    /// 移除设备（不会自动停止），返回移除的句柄
    pub async fn remove_device(&self, socket_addr: SocketAddr, slave_id: u8) -> Option<DeviceHandle> {
        let mut map = self.inner.lock().await;
        map.remove(&(socket_addr, slave_id))
    }

    /// 停止指定设备并等待其结束
    pub async fn stop_device(&self, socket_addr: SocketAddr, slave_id: u8) -> Option<Result<()>> {
        if let Some(handle) = self.remove_device(socket_addr, slave_id).await {
            Some(handle.stop_and_wait().await)
        } else {
            None
        }
    }

    /// 停止所有设备并等待结束
    pub async fn stop_all(&self) -> Result<()> {
        // 拿走所有句柄，避免在等待过程中持有锁
        let mut map = self.inner.lock().await;
        let handles: Vec<DeviceHandle> = map.drain().map(|(_, h)| h).collect();
        drop(map);

        for h in handles {
            h.stop_and_wait().await?;
        }
        Ok(())
    }

    /// 列出当前管理的设备
    pub async fn list_devices(&self) -> Vec<(SocketAddr, u8)> {
        let map = self.inner.lock().await;
        map.keys().cloned().collect()
    }
}
