use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, bail};
use axum::{
    Router,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    routing::{any, get},
};
use axum_extra::{TypedHeader, headers};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::mpsc::{self, Sender},
    time::{Instant, sleep},
};
use tracing::info;

use crate::{animation::FRAMES, cli::Args, telnet::build_frame};

#[derive(Clone)]
struct AppState {
    // 命令行参数
    args: Args,
}

pub async fn run_http(args: Args) -> anyhow::Result<()> {
    let state = AppState { args };
    let app = Router::new()
        .route("/", get(handler))
        .route("/ws", any(ws))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handler() -> &'static str {
    "Hello, World!"
}

async fn ws(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    state: State<AppState>,
) -> axum::response::Response {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };
    info!("`{user_agent}` at {addr:?} connected.");

    ws.on_upgrade(move |socket| handle_socket(socket, addr, state.args.clone()))
}

#[derive(Clone, Serialize, Deserialize)]
#[repr(u8)]
pub enum StatusCode {
    /// 初始化，请求客户端大小
    Init = 0,
    /// 处理消息，客户端返回大小，服务端发送动画
    Ok = 1,
    Error = 2,
}

/// 消息帧
#[derive(Clone, Serialize, Deserialize)]
pub struct MessageFrame {
    code: StatusCode,
    width: Option<u16>,
    height: Option<u16>,
    frame: Option<String>,
}

async fn handle_socket(socket: WebSocket, who: SocketAddr, args: Args) {
    let (mut sender, mut receiver) = socket.split();

    // 创建两个 channel，用于在应用程序和 WebSocket 之间传递消息
    // 从应用程序接收消息并发送到 WebSocket
    // let (tx_to_ws, rx_to_ws) = mpsc::channel::<MessageFrame>(128); // 应用程序 → WebSocket
    // 从 WebSocket 接收消息并发送到应用程序
    let (tx_from_ws, mut rx_from_ws) = mpsc::channel::<MessageFrame>(128); // WebSocket → 应用程序

    // 发送方向 从 channel 接受消息
    let args = args.clone();
    let mut send_task = tokio::spawn(async move {
        // 第一帧
        let msg = MessageFrame {
            code: StatusCode::Init,
            width: None,
            height: None,
            frame: None,
        };
        let msg_serialized =
            serde_json::to_string(&msg).with_context(|| "Could not serialize init message")?;
        sender
            .send(Message::Text(msg_serialized.into()))
            .await
            .with_context(|| "Could not send message")?;

        // 后续帧
        while let Some(msg) = rx_from_ws.recv().await {
            match msg.code {
                StatusCode::Ok => {
                    // 构建帧内容
                    let width = msg
                        .width
                        .ok_or_else(|| anyhow::anyhow!("Could not get width from message"))?;
                    let height = msg
                        .height
                        .ok_or_else(|| anyhow::anyhow!("Could not get height from message"))?;

                    // 发送动画帧
                    let mut frame_idx = 0;
                    let start_time = Instant::now();
                    loop {
                        let frame_data = build_frame(width, height, &args, frame_idx, start_time);

                        let msg = MessageFrame {
                            code: StatusCode::Ok,
                            width: None,
                            height: None,
                            frame: Some(frame_data),
                        };

                        let msg_serialized = serde_json::to_string(&msg)
                            .with_context(|| "Could not serialize message")?;

                        // 发送帧数据
                        sender
                            .send(Message::Text(msg_serialized.into()))
                            .await
                            .with_context(|| "Could not send message")?;

                        // 控制帧率
                        sleep(Duration::from_millis(100)).await;

                        // 下一帧
                        frame_idx = (frame_idx + 1) % FRAMES.len();
                    }
                }
                StatusCode::Error => {
                    bail!("Error received from client");
                }
                _ => continue,
            }
        }

        anyhow::Ok(())
    });

    // This second task will receive messages from client and print them on server console
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            // print message and break if instructed to do so
            process_message(msg, who, tx_from_ws.clone())
                .await
                .with_context(|| "Error processing message")?;
        }
        anyhow::Ok(())
    });

    // If any one of the tasks exit, abort the other.
    tokio::select! {
        rv_a = (&mut send_task) => {
            match rv_a {
                Ok(a) => {
                    match a {
                        Ok(_) => info!("messages sent to {who}"),
                        Err(a) => info!("Error sending messages {a:?}")
                    }
                },
                Err(a) => info!("Error sending messages {a:?}")
            }
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b) => {
                    match b {
                        Ok(_) => info!("Received messages"),
                        Err(b) => info!("Error receiving messages {b:?}")
                    }
                },
                Err(b) => info!("Error receiving messages {b:?}")
            }
            send_task.abort();
        }
    }

    // returning from the handler closes the websocket connection
    info!("Websocket context {who} destroyed");
}

/// helper to print contents of messages to stdout. Has special treatment for Close.
async fn process_message(
    msg: Message,
    who: SocketAddr,
    tx_from_ws: Sender<MessageFrame>,
) -> anyhow::Result<()> {
    match msg {
        Message::Text(t) => {
            info!(">>> {who} sent str: {t:?}");
            let msg = serde_json::from_str::<MessageFrame>(&t)?;
            tx_from_ws.send(msg).await?;
        }
        Message::Binary(d) => {
            info!(">>> {} sent {} bytes: {:?}", who, d.len(), d);
        }
        Message::Close(c) => {
            if let Some(cf) = c {
                info!(
                    ">>> {} sent close with code {} and reason `{}`",
                    who, cf.code, cf.reason
                );
            } else {
                info!(">>> {who} somehow sent close message without CloseFrame");
            }
            bail!("Could not deserialize message");
        }

        Message::Pong(v) => {
            info!(">>> {who} sent pong with {v:?}");
        }
        // You should never need to manually handle Message::Ping, as axum's websocket library
        // will do so for you automagically by replying with Pong and copying the v according to
        // spec. But if you need the contents of the pings you can see them here.
        Message::Ping(v) => {
            info!(">>> {who} sent ping with {v:?}");
        }
    }
    Ok(())
}
