use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use tokio::time::sleep;

use crate::{
    animation::{FRAMES, render_frame},
    cli::Args,
};

mod animation;
mod cli;
mod telnet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    run_standalone(&args).await?;
    Ok(())
}

// 独立模式运行
async fn run_standalone(args: &Args) -> anyhow::Result<()> {
    let mut stdout = io::stdout();

    // 终端初始化
    enable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen, cursor::Hide)?;

    // 监听退出信号
    let event_loop = tokio::spawn(async {
        loop {
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(event) = event::read()?
                && event.kind == KeyEventKind::Press
                && (event.code == KeyCode::Esc || event.code == KeyCode::Char('q'))
            {
                return anyhow::Ok(());
            }
        }
    });

    // 动画循环
    let mut frame_idx = 0;
    let start_time = Instant::now();

    loop {
        // 检查是否退出
        if event_loop.is_finished() {
            break;
        }

        // 获取终端大小
        let size = crossterm::terminal::size()?;

        // 渲染当前帧
        render_frame(FRAMES[frame_idx], size.0, size.1, args.no_clear)?;

        // 显示计数器
        if !args.no_counter {
            let elapsed = start_time.elapsed().as_secs();
            execute!(stdout, cursor::MoveTo(0, size.1 - 1))?;
            print!("Frame: {}, Time: {}s", frame_idx, elapsed);
            stdout.flush()?;
        }

        // 控制帧率
        sleep(Duration::from_millis(100)).await;

        // 检查帧限制
        if let Some(limit) = args.frames
            && frame_idx >= limit
        {
            break;
        }

        // 下一帧
        frame_idx = (frame_idx + 1) % FRAMES.len();
    }

    // 恢复终端
    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen, cursor::Show)?;

    Ok(())
}
