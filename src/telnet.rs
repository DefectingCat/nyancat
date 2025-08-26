use std::time::Duration;

use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::sleep,
};

use crate::{
    animation::{FRAMES, RenderSize, render_color},
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

        let RenderSize {
            min_col,
            max_col,
            min_row,
            max_row,
        } = RenderSize::new(client_width, client_height);

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

/// 解析Telnet客户端发送的协议命令
/// 提取窗口大小信息并更新到width和height
/// 成功获取窗口大小后返回true
fn parse_telnet_commands(data: &[u8], width: &mut u16, height: &mut u16) -> bool {
    let mut i = 0;
    let data_len = data.len();

    while i < data_len {
        // 查找Telnet命令标记(IAC)
        if data[i] == IAC && i + 1 < data_len {
            match data[i + 1] {
                // 处理子协商命令
                SB => {
                    // 确保有足够的字节进行解析
                    if i + 2 >= data_len {
                        break;
                    }

                    let option = data[i + 2];
                    i += 3; // 跳过IAC, SB, option

                    // 处理窗口大小子协商
                    if option == NAWS {
                        // NAWS需要4字节数据(宽度高8位、宽度低8位、高度高8位、高度低8位)
                        if i + 4 <= data_len {
                            *width = ((data[i] as u16) << 8) | data[i + 1] as u16;
                            *height = ((data[i + 2] as u16) << 8) | data[i + 3] as u16;

                            // 跳过数据并寻找子协商结束标记
                            i += 4;
                            while i + 1 < data_len && !(data[i] == IAC && data[i + 1] == SE) {
                                i += 1;
                            }

                            // 跳过SE标记
                            // if i + 1 < data_len {
                            //     i += 2;
                            // }

                            return true; // 成功获取窗口大小
                        }
                    }
                    // 处理终端类型子协商（仅跳过，不处理具体类型）
                    else if option == TTYPE {
                        // 跳过终端类型数据直到子协商结束
                        while i + 1 < data_len && !(data[i] == IAC && data[i + 1] == SE) {
                            i += 1;
                        }
                        // 跳过SE标记
                        if i + 1 < data_len {
                            i += 2;
                        }
                    }
                    // 其他子协商类型：直接跳到结束
                    else {
                        while i + 1 < data_len && !(data[i] == IAC && data[i + 1] == SE) {
                            i += 1;
                        }
                        if i + 1 < data_len {
                            i += 2;
                        }
                    }
                }

                // 其他Telnet命令：跳过3字节(IAC + cmd + opt)
                _ => {
                    i += 3;
                }
            }
        }
        // 非命令数据：向前移动1字节
        else {
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
