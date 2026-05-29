use std::{net::SocketAddr, sync::atomic::{AtomicUsize, Ordering}};

use anyhow::{Context, bail};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use axum_extra::{TypedHeader, headers};
use futures::{sink::SinkExt, stream::StreamExt};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tokio::{
    sync::{broadcast, mpsc::{self, Sender}},
    time::{Instant, sleep},
};
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tower_serve_static::ServeDir;
use tracing::{Span, error, info, info_span, warn};

use crate::{animation::FRAMES, cli::Args, telnet::build_frame};

/// 活跃连接计数器
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

/// Middleware for logging each HTTP request.
///
/// Uses `TraceLayer::new_for_http` with custom callbacks so that every
/// request/response is logged at INFO level (the default `DefaultOnResponse`
/// logs at DEBUG which is filtered out by the default INFO-level filter).
/// Latency is automatically formatted by tracing's `?`/`%` display.
pub fn logging_route(router: Router) -> Router {
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|req: &Request<_>| {
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
        })
        .on_response(|res: &Response, latency: std::time::Duration, _span: &Span| {
            info!(status = res.status().as_u16(), ?latency, "finished processing request");
        })
        .on_failure(
            |error: ServerErrorsFailureClass, latency: std::time::Duration, _span: &Span| {
                error!(%error, ?latency, "request failed");
            },
        );

    router.layer(trace_layer)
}

#[derive(Clone)]
struct AppState {
    args: Args,
}

static FRONTEND_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/frontend/dist");

pub async fn run_http(
    args: Args,
    mut shutdown: broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    let state = AppState { args: args.clone() };

    let service = ServeDir::new(&FRONTEND_DIR);

    let app = Router::new()
        .route("/health", get(health))
        .fallback_service(service)
        .route("/ws", any(ws))
        .with_state(state);

    let app = logging_route(app);

    let addr = format!("{}:{}", args.http_host, args.http_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind to {}", addr))?;
    info!("HTTP server listening on {}", listener.local_addr()?);

    // 使用 graceful_shutdown 支持优雅关闭
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = shutdown.recv().await;
        info!("HTTP server received shutdown signal");
    })
    .await?;

    Ok(())
}

/// 健康检查端点
async fn health() -> (StatusCode, Json<serde_json::Value>) {
    let connections = ACTIVE_CONNECTIONS.load(Ordering::Relaxed);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "active_connections": connections,
        })),
    )
}

async fn ws(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    state: State<AppState>,
) -> axum::response::Response {
    // 检查连接数限制
    if state.args.has_connection_limit() {
        let current = ACTIVE_CONNECTIONS.load(Ordering::Relaxed);
        if current >= state.args.max_connections {
            warn!(
                "Connection limit reached ({}/{}), rejecting {}",
                current, state.args.max_connections, addr
            );
            return (StatusCode::SERVICE_UNAVAILABLE, "Connection limit reached").into_response();
        }
    }

    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };
    info!("`{}` at {} connected via WebSocket", user_agent, addr);

    ws.on_upgrade(move |socket| handle_socket(socket, addr, state.args.clone()))
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug)]
#[repr(u8)]
pub enum FrameCode {
    /// 初始化，请求客户端大小
    Init = 0,
    /// 处理消息，客户端返回大小，服务端发送动画
    Ok = 1,
    Error = 2,
}

/// 消息帧
#[derive(Serialize, Deserialize)]
pub struct MessageFrame {
    code: FrameCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<String>,
}

async fn handle_socket(socket: WebSocket, who: SocketAddr, args: Args) {
    ACTIVE_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
    let _guard = ConnectionGuard;

    let frame_interval = args.frame_interval();
    let ping_interval = args.ws_ping_interval();
    let idle_timeout = args.idle_timeout();

    let (mut sender, mut receiver) = socket.split();

    // 从 WebSocket 接收消息并发送到应用程序
    let (tx_from_ws, mut rx_from_ws) = mpsc::channel::<MessageFrame>(128); // WebSocket → 应用程序

    // 发送方向 从 channel 接受消息
    let args = args.clone();
    let mut send_task: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
        // 第一帧
        let msg = MessageFrame {
            code: FrameCode::Init,
            width: None,
            height: None,
            frame: None,
        };
        let msg_serialized =
            serde_json::to_string(&msg).with_context(|| "Could not serialize init message")?;
        sender
            .send(Message::Text(msg_serialized.into()))
            .await
            .with_context(|| "Could not send init message")?;

        // 等待客户端返回初始尺寸
        let (mut width, mut height) = loop {
            if let Some(msg) = rx_from_ws.recv().await {
                match msg.code {
                    FrameCode::Ok => {
                        let w = msg
                            .width
                            .ok_or_else(|| anyhow::anyhow!("Could not get width from message"))?;
                        let h = msg
                            .height
                            .ok_or_else(|| anyhow::anyhow!("Could not get height from message"))?;
                        break (w, h);
                    }
                    FrameCode::Error => {
                        bail!("Error received from client");
                    }
                    _ => continue,
                }
            }
        };

        // 发送动画帧，同时监听 resize 消息
        let mut frame_idx = 0;
        let start_time = Instant::now();
        let mut last_activity = Instant::now();

        loop {
            // 检查帧限制
            if let Some(limit) = args.frames
                && frame_idx >= limit
            {
                info!("WebSocket {} reached frame limit {}", who, limit);
                break Ok(());
            }

            tokio::select! {
                // 定时发送下一帧
                _ = sleep(frame_interval) => {
                    last_activity = Instant::now();
                    let frame_data = build_frame(width, height, &args, frame_idx, start_time, "\r\n");

                    let msg = MessageFrame {
                        code: FrameCode::Ok,
                        width: None,
                        height: None,
                        frame: Some(frame_data),
                    };

                    let msg_serialized = serde_json::to_string(&msg)
                        .with_context(|| "Could not serialize message")?;

                    sender
                        .send(Message::Text(msg_serialized.into()))
                        .await
                        .with_context(|| "Could not send frame message")?;

                    frame_idx += 1;
                    if frame_idx >= FRAMES.len() {
                        frame_idx = 0;
                    }
                }
                // 定时发送 Ping（如果启用）
                _ = async {
                    if let Some(interval) = ping_interval {
                        sleep(interval).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    sender
                        .send(Message::Ping(vec![].into()))
                        .await
                        .with_context(|| "Could not send ping")?;
                }
                // 监听客户端 resize 消息
                maybe_msg = rx_from_ws.recv() => {
                    if let Some(msg) = maybe_msg {
                        last_activity = Instant::now();
                        match msg.code {
                            FrameCode::Ok => {
                                if let Some(w) = msg.width {
                                    width = w;
                                }
                                if let Some(h) = msg.height {
                                    height = h;
                                }
                            }
                            FrameCode::Error => {
                                bail!("Error received from client");
                            }
                            _ => {}
                        }
                    }
                }
            }

            // 检查空闲超时
            if let Some(timeout) = idle_timeout
                && last_activity.elapsed() > timeout
            {
                info!("WebSocket {} idle timeout after {:?}", who, timeout);
                break Ok(());
            }
        }
    });

    // This second task will receive messages from client and print them on server console
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Err(e) = process_message(msg, who, tx_from_ws.clone()).await {
                error!("Error processing message from {}: {:?}", who, e);
                break;
            }
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
                Err(a) => error!("Error sending task panicked {a:?}")
            }
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b) => {
                    match b {
                        Ok(_) => info!("Received messages from {who}"),
                        Err(b) => error!("Error receiving messages {b:?}")
                    }
                },
                Err(b) => error!("Error receiving task panicked {b:?}")
            }
            send_task.abort();
        }
    }

    // returning from the handler closes the websocket connection
    info!("WebSocket context {who} destroyed");
}

/// RAII guard to decrement connection count
struct ConnectionGuard;

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// helper to print contents of messages to stdout. Has special treatment for Close.
async fn process_message(
    msg: Message,
    who: SocketAddr,
    tx_from_ws: Sender<MessageFrame>,
) -> anyhow::Result<()> {
    match msg {
        Message::Text(t) => {
            let msg = serde_json::from_str::<MessageFrame>(&t)
                .with_context(|| format!("invalid JSON from {}", who))?;
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
            return Ok(());
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
