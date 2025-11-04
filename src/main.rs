// src/main.rs
mod device_manager;
mod file_watcher;

use crate::device_manager::DeviceManager;
use crate::file_watcher::{FileEventHandler, FileWatcherConfig, run_file_watcher_loop};
use anyhow::Result;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};

/// 自定义文件事件处理器
struct CustomFileEventHandler;

#[async_trait::async_trait]
impl FileEventHandler for CustomFileEventHandler {
    async fn on_file_create(&self, path: &std::path::Path) -> Result<()> {
        println!("🎉 检测到新文件: {}", path.display());
        Ok(())
    }

    async fn on_file_modify(&self, path: &std::path::Path) -> Result<()> {
        println!("📝 检测到文件修改: {}", path.display());
        Ok(())
    }

    async fn on_file_remove(&self, path: &std::path::Path) -> Result<()> {
        println!("🗑️  检测到文件删除: {}", path.display());
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 启动应用程序...");

    // 创建设备管理器
    let manager = DeviceManager::new();

    // 示例设备地址
    let addr1: SocketAddr = "127.0.0.1:502".parse().unwrap();

    // 使用 DeviceManager 的便捷方法添加 Modbus 设备
    manager.add_modbus_device(addr1, 1, 2).await?;
    println!("✅ 已添加 Modbus 设备 {} slave {}", addr1, 1);

    // 启动文件监听器
    let watch_dir = PathBuf::from("."); // 监听当前目录
    let file_handler = Arc::new(CustomFileEventHandler);

    let file_watcher_handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        let config = FileWatcherConfig {
            recursive: true, // 递归监听子目录
            ..Default::default()
        };

        println!("👀 启动文件监听器，监听目录: {}", watch_dir.display());
        run_file_watcher_loop(watch_dir, file_handler, Some(config)).await
    });

    println!("⏰ 运行 15 秒...");
    sleep(Duration::from_secs(15)).await;

    println!("🛑 正在停止所有设备...");
    manager.stop_all().await?;
    println!("✅ 已停止所有设备");

    // 停止文件监听器（通过取消任务）
    file_watcher_handle.abort();
    println!("✅ 已停止文件监听器");

    println!("👋 应用程序正常退出");
    Ok(())
}
