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

    /// 帧率（每秒帧数）
    #[arg(short = 'f', long, default_value_t = 10)]
    pub fps: u8,

    /// 不显示计数器
    #[arg(short = 'n', long = "no-counter")]
    pub no_counter: bool,

    /// 不清除屏幕
    #[arg(short = 'e', long = "no-clear")]
    pub no_clear: bool,

    /// 显示指定帧数后退出（0 = 无限）
    #[arg(short = 'F', long)]
    pub frames: Option<usize>,

    // ===== Telnet 配置 =====
    /// Telnet 服务器端口
    #[arg(short = 'p', long, default_value_t = 23)]
    pub port: u16,

    /// Telnet 绑定地址
    #[arg(long = "telnet-host", default_value = "0.0.0.0")]
    pub telnet_host: String,

    /// Telnet 默认终端宽度
    #[arg(long = "default-width", default_value_t = 80)]
    pub default_width: u16,

    /// Telnet 默认终端高度
    #[arg(long = "default-height", default_value_t = 24)]
    pub default_height: u16,

    /// Telnet 握手超时（秒）
    #[arg(long = "handshake-timeout", default_value_t = 30)]
    pub handshake_timeout: u64,

    // ===== HTTP 配置 =====
    #[cfg(feature = "http")]
    /// HTTP 服务器端口
    #[arg(long = "http-port", default_value_t = 3000)]
    pub http_port: u16,

    #[cfg(feature = "http")]
    /// HTTP 绑定地址
    #[arg(long = "http-host", default_value = "0.0.0.0")]
    pub http_host: String,

    #[cfg(feature = "http")]
    /// WebSocket Ping 间隔（秒，0 = 禁用）
    #[arg(long = "ws-ping-interval", default_value_t = 30)]
    pub ws_ping_interval: u64,

    #[cfg(feature = "http")]
    /// 连接空闲超时（秒，0 = 禁用）
    #[arg(long = "idle-timeout", default_value_t = 60)]
    pub idle_timeout: u64,

    #[cfg(feature = "http")]
    /// 最大并发连接数（0 = 无限制）
    #[arg(long = "max-connections", default_value_t = 0)]
    pub max_connections: usize,
}

impl Args {
    /// 计算帧间隔（毫秒）
    pub fn frame_interval_ms(&self) -> u64 {
        1000 / self.fps.max(1) as u64
    }

    /// 计算帧间隔 Duration
    pub fn frame_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.frame_interval_ms())
    }

    /// 握手超时 Duration
    pub fn handshake_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.handshake_timeout)
    }

    #[cfg(feature = "http")]
    /// WebSocket Ping 间隔 Duration（None 表示禁用）
    pub fn ws_ping_interval(&self) -> Option<std::time::Duration> {
        if self.ws_ping_interval == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(self.ws_ping_interval))
        }
    }

    #[cfg(feature = "http")]
    /// 空闲超时 Duration（None 表示禁用）
    pub fn idle_timeout(&self) -> Option<std::time::Duration> {
        if self.idle_timeout == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(self.idle_timeout))
        }
    }

    #[cfg(feature = "http")]
    /// 是否启用连接数限制
    pub fn has_connection_limit(&self) -> bool {
        self.max_connections > 0
    }
}
