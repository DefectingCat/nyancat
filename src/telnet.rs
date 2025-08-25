use std::time::Duration;

use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::sleep,
};

use crate::{
    animation::{FRAME_HEIGHT, FRAME_WIDTH, FRAMES},
    cli::Args,
};

// Telnet协议常量
const IAC: u8 = 255;
const DO: u8 = 253;
const WILL: u8 = 251;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

// 处理Telnet客户端
pub async fn handle_telnet_client(mut stream: TcpStream, args: &Args) -> io::Result<()> {
    let addr = stream.peer_addr()?;
    println!("New telnet connection from {}", addr);

    // Telnet握手
    let handshake = [
        IAC, WILL, SGA, // 启用SGA
        IAC, DO, TTYPE, // 要求终端类型
        IAC, DO, NAWS, // 要求窗口大小
    ];
    stream.write_all(&handshake).await?;

    // 读取客户端响应
    let mut buf = [0; 1024];
    let mut client_width = 80;
    let mut client_height = 24;

    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break, // 连接关闭
            Ok(n) => {
                // 简单处理Telnet命令（实际需要更完整的解析）
                if parse_telnet_commands(&buf[..n], &mut client_width, &mut client_height) {
                    // 命令处理完成，开始发送动画
                    break;
                }
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }
    }

    // 发送动画帧
    let mut frame_idx = 0;
    loop {
        // 渲染帧到缓冲区
        let mut frame_data = String::new();
        if !args.no_clear {
            frame_data.push_str("\x1B[2J\x1B[1;1H"); // 清屏
        }

        // 计算裁剪范围
        let term_half_width = (client_width / 2) as usize;
        let min_col = (FRAME_WIDTH.saturating_sub(term_half_width)).saturating_div(2);
        let max_col = min_col + term_half_width;
        let min_row = (FRAME_HEIGHT.saturating_sub(client_height as usize)).saturating_div(2);
        // 减去终端高度减去1，因为终端坐标系从 0 开始
        let max_row = min_row + (client_height - 1) as usize;

        // 构建帧内容
        for (y, row) in FRAMES[frame_idx].iter().enumerate() {
            if y < min_row || y >= max_row {
                continue;
            }

            for (x, c) in row.chars().enumerate() {
                if x < min_col || x >= max_col {
                    continue;
                }

                // let color = get_color_code(c);
                // frame_data.push_str(&format!("\x1B[3{}m  \x1B[0m", color));
                match c {
                    ',' => frame_data.push_str("\x1B[48;5;17m  \x1B[0m"),
                    '.' => frame_data.push_str("\x1B[48;5;231m  \x1B[0m"),
                    '\'' => frame_data.push_str("\x1B[48;5;16m  \x1B[0m"),
                    '@' => frame_data.push_str("\x1B[48;5;230m  \x1B[0m"),
                    '$' => frame_data.push_str("\x1B[48;5;175m  \x1B[0m"),
                    '-' => frame_data.push_str("\x1B[48;5;162m  \x1B[0m"),
                    '>' => frame_data.push_str("\x1B[48;5;196m  \x1B[0m"),
                    '&' => frame_data.push_str("\x1B[48;5;214m  \x1B[0m"),
                    '+' => frame_data.push_str("\x1B[48;5;226m  \x1B[0m"),
                    '#' => frame_data.push_str("\x1B[48;5;118m  \x1B[0m"),
                    '=' => frame_data.push_str("\x1B[48;5;33m  \x1B[0m"),
                    ';' => frame_data.push_str("\x1B[48;5;19m  \x1B[0m"),
                    '*' => frame_data.push_str("\x1B[48;5;240m  \x1B[0m"),
                    '%' => frame_data.push_str("\x1B[48;5;175m  \x1B[0m"),
                    _ => todo!(),
                };
            }
            frame_data.push('\n');
        }

        // 发送帧数据
        stream.write_all(frame_data.as_bytes()).await?;
        stream.flush().await?;

        // 控制帧率
        sleep(Duration::from_millis(100)).await;

        // 下一帧
        frame_idx = (frame_idx + 1) % FRAMES.len();
    }
}

// 简单解析Telnet命令
fn parse_telnet_commands(data: &[u8], width: &mut u16, height: &mut u16) -> bool {
    let mut i = 0;
    while i < data.len() {
        if data[i] == IAC && i + 2 < data.len() {
            let cmd = data[i + 1];
            let opt = data[i + 2];

            // 处理窗口大小协商
            if cmd == 250 && opt == TTYPE && i + 4 < data.len() && data[i + 3] == 0 {
                // 终端类型响应，跳过
                i += 4;
            } else if cmd == 250 && opt == NAWS && i + 6 < data.len() {
                // 窗口大小响应
                *width = ((data[i + 3] as u16) << 8) | data[i + 4] as u16;
                *height = ((data[i + 5] as u16) << 8) | data[i + 6] as u16;
                i += 7;
                return true; // 已获取窗口大小，准备发送动画
            } else {
                i += 3;
            }
        } else {
            i += 1;
        }
    }
    false
}

// 运行Telnet服务器
pub async fn run_telnet_server(args: &Args) -> io::Result<()> {
    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;
    println!("Telnet server running on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let cli_args = args.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_telnet_client(stream, &cli_args).await {
                eprintln!("Telnet client error: {}", e);
            }
        });
    }
}
