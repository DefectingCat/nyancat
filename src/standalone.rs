use std::{
    io::{self, Write},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use tokio::time::{Instant, sleep};

use crate::{
    animation::{FRAMES, NyanedTime, RenderSize, render_color},
    cli::Args,
};

/// RAII guard for terminal state restoration.
/// Ensures raw mode is disabled and cursor is shown on drop.
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

// 独立模式运行
pub async fn run_standalone(
    args: &Args,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    let _guard = TerminalGuard::new()?;
    let mut stdout = io::stdout();

    let frame_interval = args.frame_interval();

    // 监听退出信号 (spawn_blocking because crossterm events are synchronous)
    let event_loop = tokio::task::spawn_blocking(move || {
        loop {
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => {
                    if let Ok(Event::Key(event)) = event::read()
                        && event.kind == KeyEventKind::Press
                        && (event.code == KeyCode::Esc || event.code == KeyCode::Char('q'))
                    {
                        break;
                    }
                }
                Ok(false) => continue,
                Err(_) => break,
            }
        }
    });

    // 动画循环
    let mut frame_idx = 0;
    let start_time = Instant::now();

    if !args.no_clear {
        execute!(
            stdout,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
    }

    loop {
        // 检查是否退出
        if event_loop.is_finished() {
            break;
        }

        // 检查关闭信号
        if crate::shutdown::is_shutdown(&mut shutdown_rx) {
            break;
        }

        // 获取终端大小
        let size = crossterm::terminal::size()?;
        let (terminal_width, terminal_height) = size;

        let RenderSize {
            min_col,
            max_col,
            min_row,
            max_row,
        } = RenderSize::new(terminal_width, terminal_height);

        // 渲染当前帧（批量构建）
        let frame_text = build_frame_text(FrameParams {
            frame: FRAMES[frame_idx],
            min_row,
            max_row,
            min_col,
            max_col,
            args,
            start_time,
            terminal_width,
            terminal_height,
        });

        execute!(stdout, cursor::MoveTo(0, 0))?;
        stdout.write_all(frame_text.as_bytes())?;
        stdout.flush()?;

        // 控制帧率
        sleep(frame_interval).await;

        // 检查帧限制
        if let Some(limit) = args.frames
            && frame_idx + 1 >= limit
        {
            break;
        }

        // 下一帧
        frame_idx = (frame_idx + 1) % FRAMES.len();
    }

    Ok(())
}

struct FrameParams<'a> {
    frame: &'a [&'a str],
    min_row: usize,
    max_row: usize,
    min_col: usize,
    max_col: usize,
    args: &'a Args,
    start_time: Instant,
    terminal_width: u16,
    terminal_height: u16,
}

/// 构建完整帧文本（批量渲染）
fn build_frame_text(params: FrameParams) -> String {
    let FrameParams {
        frame,
        min_row,
        max_row,
        min_col,
        max_col,
        args,
        start_time,
        terminal_width,
        terminal_height,
    } = params;

    let mut output = String::new();

    // 清屏
    if !args.no_clear {
        output.push_str("\x1B[2J\x1B[1;1H");
    }

    // 帧内容
    for (y, row) in frame.iter().enumerate() {
        if y < min_row || y >= max_row {
            continue;
        }

        for (x, c) in row.chars().enumerate() {
            if x < min_col || x >= max_col {
                continue;
            }
            output.push_str(render_color(c));
        }

        // 只在非最后一行添加换行
        if y < max_row - 1 {
            output.push('\n');
        }
    }

    // 计数器
    if !args.no_counter {
        let nyaned_time = NyanedTime::new(start_time, terminal_width);
        if nyaned_time.text_len >= terminal_width.into() {
            output.push('\n');
            output.push_str(&nyaned_time.nyaned);
        } else {
            let counter_line = (terminal_height.saturating_sub(1)).max(1);
            output.push_str(&format!("\x1B[{};1H", counter_line));
            output.push_str(&nyaned_time.counter_text);
        }
    }

    output
}
