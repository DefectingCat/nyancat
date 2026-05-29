use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::info;

/// 优雅关闭信号管理器
///
/// 监听系统信号（SIGINT/SIGTERM），并通过广播通知所有子任务
#[derive(Clone)]
pub struct Shutdown {
    tx: Arc<broadcast::Sender<()>>,
}

impl Shutdown {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(1);
        Self { tx: Arc::new(tx) }
    }

    /// 订阅关闭信号
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.tx.subscribe()
    }

    /// 触发关闭（内部使用）
    fn notify(&self) {
        let _ = self.tx.send(());
    }

    /// 监听系统信号并触发关闭
    ///
    /// 支持 Ctrl+C (SIGINT) 和 SIGTERM (Unix)
    pub async fn handle_signals(&self) {
        let ctrl_c = tokio::signal::ctrl_c();

        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut sigterm = signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");

            tokio::select! {
                _ = ctrl_c => {
                    info!("Received SIGINT (Ctrl+C), shutting down...");
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, shutting down...");
                }
            }
        }

        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
            info!("Received Ctrl+C, shutting down...");
        }

        self.notify();
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

/// 检查是否收到关闭信号（非阻塞）
pub fn is_shutdown(rx: &mut broadcast::Receiver<()>) -> bool {
    matches!(rx.try_recv(), Ok(()) | Err(broadcast::error::TryRecvError::Closed))
}
