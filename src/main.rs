use anyhow::Context;
use clap::Parser;

use crate::cli::Args;

mod animation;
mod cli;
mod http;
mod logging;
mod standalone;
mod telnet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    logging::init_logger().with_context(|| "init logger failed")?;

    if args.telnet {
        telnet::run_telnet_server(&args).await?;
    }

    if args.http {
        http::run_http().await?;
    }

    standalone::run_standalone(&args).await?;
    Ok(())
}
