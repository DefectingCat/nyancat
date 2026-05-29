use anyhow::Context;
use clap::Parser;

use crate::cli::Args;

mod animation;
mod cli;
#[cfg(feature = "http")]
mod http;
mod logging;
mod shutdown;
mod standalone;
mod telnet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    logging::init_logger().with_context(|| "init logger failed")?;

    // 初始化优雅关闭信号
    let shutdown = shutdown::Shutdown::new();
    let shutdown_handle = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { shutdown.handle_signals().await }
    });

    if args.telnet {
        telnet::run_telnet_server(&args, shutdown.subscribe()).await?;
    } else {
        #[cfg(feature = "http")]
        if args.http {
            http::run_http(args, shutdown.subscribe()).await?;
            return Ok(());
        }

        standalone::run_standalone(&args, shutdown.subscribe()).await?;
    }

    // 等待信号处理器结束（实际上不会，因为信号处理器是无限循环）
    // 但在 graceful shutdown 完成后可以清理
    let _ = shutdown_handle.await;

    Ok(())
}
