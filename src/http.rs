use std::{fmt::Display, net::SocketAddr, time::Duration};

use anyhow::{Context, bail};
use axum::{
    Router,
    body::Bytes,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, Request},
    response::Response,
    routing::{any, get},
};
use axum_extra::{TypedHeader, headers};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tokio::{
    sync::mpsc::{self, Sender},
    time::{Instant, sleep},
};
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::{Span, error, info, info_span};

use crate::{animation::FRAMES, cli::Args, telnet::build_frame};

/// Format request latency and status message
/// return a string
fn format_latency(latency: Duration, status: impl Display) -> String {
    let micros = latency.as_micros();
    let millis = latency.as_millis();
    if micros >= 1000 {
        format!("{status} {millis}ms")
    } else {
        format!("{status} {micros}μs")
    }
}

/// Middleware for logging each request.
///
/// This middleware will calculate each request latency
/// and add request's information to each info_span.
pub fn logging_route(router: Router) -> Router {
    let make_span = |req: &Request<_>| {
        let unknown = &HeaderValue::from_static("Unknown");
        let empty = &HeaderValue::from_static("");
        let headers = req.headers();
        let ua = headers
            .get("User-Agent")
            .unwrap_or(unknown)
            .to_str()
            .unwrap_or("Unknown");
        let host = headers.get("Host").unwrap_or(empty).to_str().unwrap_or("");
        info_span!("HTTP", method = ?req.method(), host, uri = ?req.uri(), ua)
    };

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(make_span)
        .on_request(|_req: &Request<_>, _span: &Span| {})
        .on_response(|res: &Response, latency: Duration, _span: &Span| {
            info!("{}", format_latency(latency, res.status()));
        })
        .on_body_chunk(|_chunk: &Bytes, _latency: Duration, _span: &Span| {})
        .on_eos(|_trailers: Option<&HeaderMap>, _stream_duration: Duration, _span: &Span| {})
        .on_failure(
            |error: ServerErrorsFailureClass, latency: Duration, _span: &Span| {
                error!("{}", format_latency(latency, error));
            },
        );

    router.layer(trace_layer)
}

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

    let app = logging_route(app);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    info!("listening on {}", listener.local_addr()?);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
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
    info!("`{user_agent:?}` at {addr:?} connected.");
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };
    info!("`{user_agent}` at {addr:?} connected.");

    ws.on_upgrade(move |socket| handle_socket(socket, addr, state.args.clone()))
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug)]
#[repr(u8)]
pub enum StatusCode {
    /// 初始化，请求客户端大小
    Init = 0,
    /// 处理消息，客户端返回大小，服务端发送动画
    Ok = 1,
    Error = 2,
}

/// 消息帧
#[derive(Serialize, Deserialize)]
pub struct MessageFrame {
    code: StatusCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<String>,
}

async fn handle_socket(socket: WebSocket, who: SocketAddr, args: Args) {
    let (mut sender, mut receiver) = socket.split();

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
                        Err(a) => error!("Error sending messages {a:?}")
                    }
                },
                Err(a) => error!("Error sending messages {a:?}")
            }
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b) => {
                    match b {
                        Ok(_) => info!("Received messages"),
                        Err(b) => error!("Error receiving messages {b:?}")
                    }
                },
                Err(b) => error!("Error receiving messages {b:?}")
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
