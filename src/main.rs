use clap::Parser;

use crate::cli::Args;

mod animation;
mod cli;
mod standalone;
mod telnet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.telnet {
        telnet::run_telnet_server(&args).await?;
    } else {
        standalone::run_standalone(&args).await?;
    }

    Ok(())
}
