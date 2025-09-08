use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about = "Nyancat !!!", long_about = None)]
pub struct Args {
    /// telnet 模式
    #[arg(short, long)]
    pub telnet: bool,

    #[cfg(feature = "http")]
    /// http 模式
    #[arg(short = 'H', long)]
    pub http: bool,

    /// 不显示计数器
    #[arg(short = 'n', long = "no-counter")]
    pub no_counter: bool,

    /// 不清除屏幕
    #[arg(short = 'e', long = "no-clear")]
    pub no_clear: bool,

    /// 显示指定帧数后退出
    #[arg(short, long)]
    pub frames: Option<usize>,

    /// Telnet服务器端口
    #[arg(short = 'p', long, default_value_t = 23)]
    pub port: u16,
}
