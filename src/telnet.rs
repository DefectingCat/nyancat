use std::time::Duration;

use anyhow::Context;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast,
    time::{Instant, sleep},
};
use tracing::{error, info};

use crate::{
    animation::{FRAMES, NyanedTime, RenderSize, render_color},
    cli::Args,
};

// Telnet协议常量
const IAC: u8 = 255;
const DO: u8 = 253;
const WILL: u8 = 251;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;
const SB: u8 = 250; // 子协商开始
const SE: u8 = 240; // 子协商结束

pub fn build_frame(
    width: u16,
    height: u16,
    args: &Args,
    frame_idx: usize,
    start_time: Instant,
    newline: &str,
) -> String {
    // 渲染帧到缓冲区
    let mut frame_data = String::new();
    if !args.no_clear {
        frame_data.push_str("\x1B[2J\x1B[1;1H"); // 清屏
    }

    let RenderSize {
        min_col,
        max_col,
        min_row,
        max_row,
    } = RenderSize::new(width, height);

    // 构建帧内容
    for (y, row) in FRAMES[frame_idx].iter().enumerate() {
        if y < min_row || y >= max_row {
            continue;
        }

        for (x, c) in row.chars().enumerate() {
            if x < min_col || x >= max_col {
                continue;
            }

            frame_data.push_str(render_color(c));
        }

        frame_data.push_str(newline);
    }

    // 显示计数器
    if !args.no_counter {
        let nyaned_time = NyanedTime::new(start_time, width);
        if nyaned_time.text_len >= width.into() {
            frame_data.push_str(&nyaned_time.nyaned);
        } else {
            frame_data.push_str(&nyaned_time.counter_text);
        }
    }

    frame_data
}

// 处理Telnet客户端
async fn handle_telnet_client(
    mut stream: TcpStream,
    args: &Args,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    let addr = stream.peer_addr().context("get peer address")?;
    info!("New telnet connection from {}", addr);

    let frame_interval = args.frame_interval();
    let handshake_timeout = args.handshake_timeout();

    // Telnet握手
    let handshake = [
        IAC, WILL, SGA, // 启用SGA
        IAC, DO, TTYPE, // 要求终端类型
        IAC, DO, NAWS, // 要求窗口大小
    ];
    stream.write_all(&handshake).await.context("send handshake")?;

    // 读取客户端响应（带累积缓冲区处理跨read的Telnet协商）
    let mut buf = [0; 1024];
    let mut client_width = args.default_width;
    let mut client_height = args.default_height;
    let mut accum = Vec::new();
    let mut handshake_done = false;

    loop {
        // 检查关闭信号
        if crate::shutdown::is_shutdown(&mut shutdown_rx) {
            info!("Telnet client {} shutting down due to signal", addr);
            return Ok(());
        }

        match tokio::time::timeout(handshake_timeout, stream.read(&mut buf)).await {
            Ok(Ok(0)) => break, // 连接关闭
            Ok(Ok(n)) => {
                accum.extend_from_slice(&buf[..n]);
                let (consumed, found) =
                    parse_telnet_commands(&accum, &mut client_width, &mut client_height);
                accum.drain(..consumed);
                if found {
                    handshake_done = true;
                    break;
                }
            }
            Ok(Err(e)) => {
                error!("Telnet read error from {}: {}", addr, e);
                return Err(e.into());
            }
            Err(_) => {
                error!("Telnet handshake timeout from {}", addr);
                break; // 使用默认值继续
            }
        }
    }

    if handshake_done {
        info!(
            "Telnet client {} negotiated size: {}x{}",
            addr, client_width, client_height
        );
    } else {
        info!(
            "Telnet client {} using default size: {}x{}",
            addr, client_width, client_height
        );
    }

    // 发送动画帧
    let mut frame_idx = 0;
    let start_time = Instant::now();

    loop {
        // 检查关闭信号
        if crate::shutdown::is_shutdown(&mut shutdown_rx) {
            info!("Telnet client {} shutting down due to signal", addr);
            break;
        }

        let frame_data = build_frame(client_width, client_height, args, frame_idx, start_time, "\n");

        // 发送帧数据
        stream
            .write_all(frame_data.as_bytes())
            .await
            .context("send frame")?;
        stream.flush().await.context("flush stream")?;

        // 控制帧率
        sleep(frame_interval).await;

        // 检查帧限制
        if let Some(limit) = args.frames
            && frame_idx + 1 >= limit
        {
            info!("Telnet client {} reached frame limit {}", addr, limit);
            break;
        }

        // 下一帧
        frame_idx = (frame_idx + 1) % FRAMES.len();
    }

    Ok(())
}

/// 解析Telnet客户端发送的协议命令
/// 提取窗口大小信息并更新到width和height
/// 返回 (已消费的字节数, 是否成功获取窗口大小)
fn parse_telnet_commands(data: &[u8], width: &mut u16, height: &mut u16) -> (usize, bool) {
    let mut i = 0;
    let data_len = data.len();
    let mut found = false;

    while i < data_len {
        // 查找Telnet命令标记(IAC)
        if data[i] == IAC && i + 1 < data_len {
            match data[i + 1] {
                // 处理子协商命令
                SB => {
                    // 确保有足够的字节进行解析
                    if i + 2 >= data_len {
                        return (i, found); // 数据不完整，保留从 i 开始的数据
                    }

                    let option = data[i + 2];
                    // 在剩余数据中查找 IAC SE 结束标记
                    let mut j = i + 3;
                    while j + 1 < data_len {
                        if data[j] == IAC && data[j + 1] == SE {
                            break;
                        }
                        j += 1;
                    }
                    if j + 1 >= data_len {
                        // 未找到完整的子协商结束标记，数据不完整
                        return (i, found);
                    }

                    // 处理窗口大小子协商
                    if option == NAWS && j - (i + 3) >= 4 {
                        *width = ((data[i + 3] as u16) << 8) | data[i + 4] as u16;
                        *height = ((data[i + 5] as u16) << 8) | data[i + 6] as u16;
                        found = true;
                    }

                    // 跳过整个子协商块
                    i = j + 2;
                }

                // 其他Telnet命令：跳过3字节(IAC + cmd + opt)
                _ => {
                    if i + 2 < data_len {
                        i += 3;
                    } else {
                        return (i, found); // 数据不完整
                    }
                }
            }
        }
        // 非命令数据：向前移动1字节
        else {
            i += 1;
        }
    }

    (i, found)
}

// 运行Telnet服务器
pub async fn run_telnet_server(
    args: &Args,
    shutdown: broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", args.telnet_host, args.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind to {}", addr))?;
    info!("Telnet server running on {}", addr);

    let mut shutdown_rx = shutdown;

    loop {
        // 检查关闭信号
        if crate::shutdown::is_shutdown(&mut shutdown_rx) {
            info!("Telnet server shutting down");
            break;
        }

        // 使用 timeout 避免阻塞在 accept 上
        let accept_timeout = Duration::from_secs(1);
        let result = tokio::time::timeout(accept_timeout, listener.accept()).await;

        let (stream, _) = match result {
            Ok(Ok(conn)) => conn,
            Ok(Err(e)) => {
                error!("Telnet accept error: {}", e);
                continue;
            }
            Err(_) => continue, // timeout, check shutdown again
        };

        let cli_args = args.clone();
        let client_shutdown = shutdown_rx.resubscribe();
        tokio::spawn(async move {
            if let Err(e) = handle_telnet_client(stream, &cli_args, client_shutdown).await {
                error!("Telnet client error: {}", e);
            }
        });
    }

    Ok(())
}
