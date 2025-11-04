#![allow(dead_code)]
#![allow(unused_imports)]

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

/// 文件事件类型
#[derive(Debug, Clone)]
pub enum FileEvent {
    /// 文件创建
    Create(PathBuf),
    /// 文件修改
    Modify(PathBuf),
    /// 文件删除
    Remove(PathBuf),
    /// 其他事件
    Other(PathBuf, EventKind),
}

/// 文件监听器配置
#[derive(Debug, Clone)]
pub struct FileWatcherConfig {
    /// 是否递归监听子目录
    pub recursive: bool,
    /// 监听的事件类型
    pub event_types: Vec<EventKind>,
    /// 忽略的文件模式（glob 模式）
    pub ignore_patterns: Vec<String>,
}

impl Default for FileWatcherConfig {
    fn default() -> Self {
        Self {
            recursive: true,
            event_types: vec![
                EventKind::Create(notify::event::CreateKind::Any),
                EventKind::Modify(notify::event::ModifyKind::Any),
                EventKind::Remove(notify::event::RemoveKind::Any),
            ],
            ignore_patterns: vec![],
        }
    }
}

/// 文件监听器
pub struct FileWatcher {
    /// 内部监听器
    watcher: RecommendedWatcher,
    /// 事件接收通道
    event_receiver: mpsc::Receiver<Result<Event, notify::Error>>,
    /// 监听路径列表
    watched_paths: Arc<Mutex<HashMap<PathBuf, RecursiveMode>>>,
}

impl FileWatcher {
    /// 创建新的文件监听器
    pub fn new() -> Result<Self> {
        let (event_sender, event_receiver) = mpsc::channel(100);

        // 使用阻塞方式发送事件，避免 Tokio 运行时问题
        let watcher = RecommendedWatcher::new(
            move |res| {
                let sender = event_sender.clone();
                // 使用阻塞发送，避免在非 Tokio 上下文中使用 await
                if let Err(e) = sender.blocking_send(res) {
                    eprintln!("发送文件事件失败: {}", e);
                }
            },
            Config::default(),
        )?;

        Ok(Self {
            watcher,
            event_receiver,
            watched_paths: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 添加监听路径
    pub async fn watch<P: AsRef<Path>>(&mut self, path: P, config: &FileWatcherConfig) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let mode = if config.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        self.watcher.watch(&path, mode)?;

        let mut watched_paths = self.watched_paths.lock().await;
        watched_paths.insert(path, mode);

        Ok(())
    }

    /// 移除监听路径
    pub async fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        self.watcher.unwatch(&path)?;

        let mut watched_paths = self.watched_paths.lock().await;
        watched_paths.remove(&path);

        Ok(())
    }

    /// 获取下一个文件事件
    pub async fn next_event(&mut self) -> Option<Vec<FileEvent>> {
        match self.event_receiver.recv().await {
            Some(Ok(event)) => {
                // 将 notify 事件转换为我们的 FileEvent 类型
                let file_events: Vec<FileEvent> = event
                    .paths
                    .into_iter()
                    .map(|path| match event.kind {
                        EventKind::Create(_) => FileEvent::Create(path),
                        EventKind::Modify(_) => FileEvent::Modify(path),
                        EventKind::Remove(_) => FileEvent::Remove(path),
                        other_kind => FileEvent::Other(path, other_kind),
                    })
                    .collect();

                Some(file_events)
            }
            Some(Err(e)) => {
                eprintln!("文件监听错误: {}", e);
                None
            }
            None => None,
        }
    }

    /// 获取当前监听的路径列表
    pub async fn watched_paths(&self) -> Vec<PathBuf> {
        let watched_paths = self.watched_paths.lock().await;
        watched_paths.keys().cloned().collect()
    }

    /// 停止所有监听
    pub async fn stop_all(&mut self) -> Result<()> {
        let watched_paths: Vec<PathBuf> = {
            let paths = self.watched_paths.lock().await;
            paths.keys().cloned().collect()
        };

        for path in watched_paths {
            if let Err(e) = self.watcher.unwatch(&path) {
                eprintln!("停止监听路径 {} 失败: {}", path.display(), e);
            }
        }

        let mut watched_paths = self.watched_paths.lock().await;
        watched_paths.clear();

        Ok(())
    }
}

/// 便捷函数：创建并启动文件监听器
pub async fn start_file_watcher<P: AsRef<Path>>(path: P, config: Option<FileWatcherConfig>) -> Result<FileWatcher> {
    let config = config.unwrap_or_default();
    let mut watcher = FileWatcher::new()?;
    watcher.watch(path, &config).await?;
    Ok(watcher)
}

/// 文件监听处理器 trait
#[async_trait::async_trait]
pub trait FileEventHandler: Send + Sync {
    /// 处理文件创建事件
    async fn on_file_create(&self, path: &Path) -> Result<()>;

    /// 处理文件修改事件
    async fn on_file_modify(&self, path: &Path) -> Result<()>;

    /// 处理文件删除事件
    async fn on_file_remove(&self, path: &Path) -> Result<()>;

    /// 处理其他文件事件
    async fn on_file_other(&self, path: &Path, kind: EventKind) -> Result<()> {
        println!("收到未处理的文件事件: {:?} - {:?}", path, kind);
        Ok(())
    }
}

/// 简单的文件事件处理器实现
pub struct SimpleFileEventHandler;

#[async_trait::async_trait]
impl FileEventHandler for SimpleFileEventHandler {
    async fn on_file_create(&self, path: &Path) -> Result<()> {
        println!("文件创建: {}", path.display());
        Ok(())
    }

    async fn on_file_modify(&self, path: &Path) -> Result<()> {
        println!("文件修改: {}", path.display());
        Ok(())
    }

    async fn on_file_remove(&self, path: &Path) -> Result<()> {
        println!("文件删除: {}", path.display());
        Ok(())
    }
}

/// 运行文件监听循环
pub async fn run_file_watcher_loop<P: AsRef<Path>>(
    path: P,
    handler: Arc<dyn FileEventHandler>,
    config: Option<FileWatcherConfig>,
) -> Result<()> {
    let mut watcher = start_file_watcher(path, config).await?;

    println!("文件监听器已启动，开始监听文件事件...");

    loop {
        if let Some(events) = watcher.next_event().await {
            for event in events {
                match event {
                    FileEvent::Create(path) => {
                        if let Err(e) = handler.on_file_create(&path).await {
                            eprintln!("处理文件创建事件失败: {}", e);
                        }
                    }
                    FileEvent::Modify(path) => {
                        if let Err(e) = handler.on_file_modify(&path).await {
                            eprintln!("处理文件修改事件失败: {}", e);
                        }
                    }
                    FileEvent::Remove(path) => {
                        if let Err(e) = handler.on_file_remove(&path).await {
                            eprintln!("处理文件删除事件失败: {}", e);
                        }
                    }
                    FileEvent::Other(path, kind) => {
                        if let Err(e) = handler.on_file_other(&path, kind).await {
                            eprintln!("处理其他文件事件失败: {}", e);
                        }
                    }
                }
            }
        } else {
            // 通道关闭，退出循环
            break;
        }
    }

    println!("文件监听器已停止");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_file_watcher_creation() {
        let watcher = FileWatcher::new();
        assert!(watcher.is_ok());
    }

    #[tokio::test]
    async fn test_file_watcher_watch() {
        let temp_dir = tempdir().unwrap();
        let mut watcher = FileWatcher::new().unwrap();

        let config = FileWatcherConfig::default();
        let result = watcher.watch(temp_dir.path(), &config).await;

        assert!(result.is_ok());

        let watched_paths = watcher.watched_paths().await;
        assert!(watched_paths.contains(&temp_dir.path().to_path_buf()));
    }
}
