use std::{
    io::{self, Write},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use tokio::time::{Instant, sleep};

use crate::{
    animation::{FRAMES, NyanedTime, RenderSize, render_color},
    cli::Args,
};

// 独立模式运行
pub async fn run_standalone(args: &Args) -> anyhow::Result<()> {
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

        // 获取终端大小
        let size = crossterm::terminal::size()?;
        let (terminal_width, terminal_height) = size;

        let RenderSize {
            min_col,
            max_col,
            min_row,
            max_row,
        } = RenderSize::new(terminal_width, terminal_height);

        // 渲染当前帧
        render_frame(FRAMES[frame_idx], min_row, max_row, min_col, max_col)?;

        // 显示计数器
        if !args.no_counter {
            let nyaned_time = NyanedTime::new(start_time, terminal_width);
            if nyaned_time.text_len >= terminal_width.into() {
                execute!(stdout, cursor::MoveTo(0, size.1 - 1))?;
                print!("{}", nyaned_time.nyaned);
            } else {
                execute!(stdout, cursor::MoveTo(0, size.1))?;
                print!("{}", nyaned_time.counter_text);
            }
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

/// 渲染帧到终端
pub fn render_frame(
    frame: &[&str],
    min_row: usize,
    max_row: usize,
    min_col: usize,
    max_col: usize,
) -> io::Result<()> {
    let mut stdout = io::stdout();

    execute!(stdout, cursor::MoveTo(0, 0))?;

    // 渲染帧内容
    // 行
    for (y, row) in frame.iter().enumerate() {
        if y < min_row || y >= max_row {
            continue;
        }

        let mut line = String::new();
        // 列
        for (x, c) in row.chars().enumerate() {
            if x < min_col || x >= max_col {
                continue;
            }

            line.push_str(render_color(c));
        }
        // 渲染的行数减去最小行数，就是跳过的行
        execute!(stdout, cursor::MoveTo(0, (y - min_row) as u16))?;
        println!("{}", line);
    }

    stdout.flush()?;
    Ok(())
}
