//! WebSocket 代理：服务端手动完成 101 握手，上游用 tokio-tungstenite 直连，
//! 双向转发并逐帧记录。
//!
//! 流程：
//! 1. 代理收到带 `Upgrade: websocket` 的请求（明文 ws:// 或解密后的 wss://）
//! 2. 用 tokio-tungstenite 直连真实服务器（wss 走 rustls）
//! 3. 计算握手 key，向客户端返回 101；连接升级后两端都转为 WebSocketStream
//! 4. select 双向转发，每帧记录（文本帧存内容，二进制帧存长度）

use crate::events::EventHub;
use crate::models::{now_ms, RequestMeta, WsFrame, WS_DIR_C2S, WS_DIR_S2C};
use crate::storage::TrafficStore;
use base64::Engine;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::Response;
use sha1::{Digest, Sha1};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// 单帧文本 payload 记录上限。
const FRAME_TEXT_MAX: usize = 32 * 1024;
/// 上游连接超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// WebSocket 握手 key 的 magic GUID（RFC 6455）。
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// 计算 Sec-WebSocket-Accept。
pub fn accept_key(client_key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(client_key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

/// 构造给客户端的 101 握手响应。
pub fn switch_response(client_key: &str, protocol: Option<&str>) -> Response<Full<Bytes>> {
    let mut builder = Response::builder()
        .status(101)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Accept", accept_key(client_key));
    if let Some(p) = protocol {
        builder = builder.header("Sec-WebSocket-Protocol", p);
    }
    builder.body(Full::new(Bytes::new())).unwrap()
}

/// 上游 WebSocket 流类型。
pub type UpstreamWs = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 连接上游 WebSocket 服务器。
///
/// `ws_url` 形如 `ws://host:port/path` 或 `wss://host/path`；
/// `orig_headers` 为客户端原始请求头（透传认证/Cookie/子协议等）。
/// 返回 (流, 上游协商出的子协议)。
pub async fn connect_upstream(
    ws_url: &str,
    orig_headers: &[(String, String)],
) -> Result<(UpstreamWs, Option<String>), String> {
    let mut builder = http::Request::builder().method("GET").uri(ws_url);

    // 透传安全头（连接管理类 header 由 tungstenite 自行生成）
    const SKIP: &[&str] = &[
        "host",
        "connection",
        "upgrade",
        "content-length",
        "content-type",
        "proxy-connection",
        "sec-websocket-key",
        "sec-websocket-version",
        "sec-websocket-extensions",
    ];
    for (k, v) in orig_headers {
        let kl = k.to_lowercase();
        if SKIP.contains(&kl.as_str()) {
            continue;
        }
        builder = builder.header(k.as_str(), v.as_str());
    }

    let req = builder.body(()).map_err(|e| format!("构造上游请求失败：{e}"))?;

    let (stream, resp) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| "连接上游 WebSocket 超时".to_string())?
        .map_err(|e| format!("连接上游 WebSocket 失败：{e}"))?;

    let protocol = resp
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    Ok((stream, protocol))
}

/// 双向转发并逐帧记录；连接结束后更新记录并推送事件。
#[allow(clippy::too_many_arguments)]
pub async fn pump_and_record<S1, S2>(
    record_id: String,
    started_at: u128,
    initial_meta: RequestMeta,
    store: Arc<dyn TrafficStore>,
    hub: EventHub,
    client_ws: WebSocketStream<S1>,
    upstream_ws: WebSocketStream<S2>,
) where
    S1: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S2: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut client_tx, mut client_rx) = client_ws.split();
    let (mut up_tx, mut up_rx) = upstream_ws.split();

    let mut seq: u64 = 0;
    let mut recorded: u64 = 0;
    let mut had_error = false;

    loop {
        tokio::select! {
            msg = client_rx.next() => {
                match msg {
                    Some(Ok(m)) => {
                        seq += 1;
                        record_frame(&store, &hub, &record_id, seq, WS_DIR_C2S, &m);
                        recorded += 1;
                        if up_tx.send(m).await.is_err() {
                            had_error = true;
                            break;
                        }
                    }
                    Some(Err(_)) => {
                        had_error = true;
                        break;
                    }
                    None => break,
                }
            }
            msg = up_rx.next() => {
                match msg {
                    Some(Ok(m)) => {
                        seq += 1;
                        record_frame(&store, &hub, &record_id, seq, WS_DIR_S2C, &m);
                        recorded += 1;
                        if client_tx.send(m).await.is_err() {
                            had_error = true;
                            break;
                        }
                    }
                    Some(Err(_)) => {
                        had_error = true;
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    // 优雅关闭两端
    let _ = client_tx.send(Message::Close(None)).await;
    let _ = up_tx.send(Message::Close(None)).await;

    // 更新记录：最终耗时 / 帧数 / 错误
    let duration = (now_ms().saturating_sub(started_at)) as u128;
    let error = if had_error {
        Some("WebSocket 连接异常断开".to_string())
    } else {
        None
    };
    store.finish_websocket(&record_id, duration, error.clone());

    let mut meta = initial_meta;
    meta.duration_ms = duration;
    meta.ws_frame_count = recorded;
    meta.error = error;
    hub.push_update(meta);
}

/// 按 UTF-8 字符边界安全截断。
fn safe_truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 记录单帧：文本帧保存截断后的内容，二进制帧仅保存长度。
fn record_frame(
    store: &Arc<dyn TrafficStore>,
    hub: &EventHub,
    record_id: &str,
    seq: u64,
    dir: u8,
    msg: &Message,
) {
    let (opcode, payload_len, payload_text) = match msg {
        Message::Text(t) => {
            let len = t.len() as u64;
            let text = if t.len() > FRAME_TEXT_MAX {
                Some(format!(
                    "{}…（已截断，共 {} 字节）",
                    safe_truncate(t.as_str(), FRAME_TEXT_MAX),
                    len
                ))
            } else {
                Some(t.to_string())
            };
            ("text".to_string(), len, text)
        }
        Message::Binary(b) => ("binary".to_string(), b.len() as u64, None),
        Message::Ping(p) => ("ping".to_string(), p.len() as u64, None),
        Message::Pong(p) => ("pong".to_string(), p.len() as u64, None),
        Message::Close(_) => ("close".to_string(), 0, None),
        Message::Frame(_) => ("frame".to_string(), 0, None),
    };

    let frame = WsFrame {
        seq,
        dir,
        opcode,
        payload_len,
        payload_text,
        ts_ms: now_ms(),
    };
    store.insert_frame(record_id, frame.clone());
    hub.push_frame(record_id, frame);
}
