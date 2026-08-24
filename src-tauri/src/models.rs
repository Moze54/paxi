//! 流量记录的数据结构（存储见 storage 模块）。

use serde::{Deserialize, Serialize};

/// WebSocket 帧方向。
pub const WS_DIR_C2S: u8 = 0; // 客户端 → 服务端
pub const WS_DIR_S2C: u8 = 1; // 服务端 → 客户端

/// 单条 WebSocket 帧。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsFrame {
    /// 该连接内的序号（两个方向共享递增）
    pub seq: u64,
    /// 方向：0 = 客户端→服务端，1 = 服务端→客户端
    pub dir: u8,
    /// opcode：text / binary / ping / pong / close
    pub opcode: String,
    /// payload 字节数
    pub payload_len: u64,
    /// 文本帧的 payload（截断存储），二进制帧为 None
    pub payload_text: Option<String>,
    /// 时间戳（epoch 毫秒）
    pub ts_ms: u128,
}

/// 单条抓包记录的完整数据结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    /// 唯一 id
    pub id: String,
    /// 发起请求的客户端 IP（若有）
    pub client_ip: Option<String>,
    /// 来源进程名（本机进程识别，仅 Windows；手机无）
    #[serde(default)]
    pub client_process: Option<String>,
    /// 请求方法，如 GET / POST
    pub method: String,
    /// 完整 URL
    pub url: String,
    /// 主机名
    pub host: String,
    /// 协议 http / https / ws / wss
    pub scheme: String,
    /// 状态码
    pub status: u16,
    /// 请求头
    pub request_headers: Vec<(String, String)>,
    /// 响应头
    pub response_headers: Vec<(String, String)>,
    /// 请求体（文本展示形式，可能被截断）
    pub request_body: Option<String>,
    /// 响应体（文本展示形式，可能被截断）
    pub response_body: Option<String>,
    /// 请求体大小（原始字节数）
    pub request_body_size: u64,
    /// 响应体大小（原始字节数）
    pub response_body_size: u64,
    /// 响应 Content-Type
    pub content_type: Option<String>,
    /// 耗时（毫秒）
    pub duration_ms: u128,
    /// 开始时间（epoch 毫秒）
    pub started_at: u128,
    /// 错误信息（若有）
    pub error: Option<String>,
    /// 是否为 WebSocket
    pub is_websocket: bool,
    /// WebSocket 帧数（实时累计）
    pub ws_frame_count: u64,
    /// 命中的规则名（若有）
    #[serde(default)]
    pub matched_rule: Option<String>,
    /// 是否为重放请求（Replay 产生）
    #[serde(default)]
    pub is_replay: bool,
    /// 是否为 TLS 直通（不解密，仅转发；App 有证书校验时使用）
    #[serde(default)]
    pub passthrough: bool,
}

/// 会话列表项（轻量，用于列表展示）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMeta {
    pub id: String,
    pub method: String,
    pub url: String,
    pub host: String,
    pub scheme: String,
    pub status: u16,
    pub duration_ms: u128,
    pub started_at: u128,
    pub is_websocket: bool,
    pub ws_frame_count: u64,
    pub error: Option<String>,
    pub client_ip: Option<String>,
    #[serde(default)]
    pub client_process: Option<String>,
    pub request_body_size: u64,
    pub response_body_size: u64,
    pub content_type: Option<String>,
    #[serde(default)]
    pub is_replay: bool,
    #[serde(default)]
    pub passthrough: bool,
}

impl From<&RequestRecord> for RequestMeta {
    fn from(r: &RequestRecord) -> Self {
        RequestMeta {
            id: r.id.clone(),
            method: r.method.clone(),
            url: r.url.clone(),
            host: r.host.clone(),
            scheme: r.scheme.clone(),
            status: r.status,
            duration_ms: r.duration_ms,
            started_at: r.started_at,
            is_websocket: r.is_websocket,
            ws_frame_count: r.ws_frame_count,
            error: r.error.clone(),
            client_ip: r.client_ip.clone(),
            request_body_size: r.request_body_size,
            response_body_size: r.response_body_size,
            content_type: r.content_type.clone(),
            client_process: r.client_process.clone(),
            is_replay: r.is_replay,
            passthrough: r.passthrough,
        }
    }
}

/// 当前时间（epoch 毫秒）。
pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// body 截断上限（防止超大响应撑爆内存）。
const MAX_BODY_BYTES: usize = 512 * 1024; // 512 KB

/// 将字节转为可安全存储的字符串（优先 UTF-8，失败则用 lossy），超过上限则截断。
pub fn body_to_string(bytes: &[u8]) -> String {
    let truncated: Vec<u8> = if bytes.len() > MAX_BODY_BYTES {
        bytes[..MAX_BODY_BYTES].to_vec()
    } else {
        bytes.to_vec()
    };
    String::from_utf8_lossy(&truncated).to_string()
}

/// 根据 Content-Encoding 解压响应体（gzip / deflate / br）。
/// 解压后再调用 body_to_string 转文本。
pub fn decode_body(bytes: &[u8], content_encoding: Option<&str>) -> Vec<u8> {
    let encoding = content_encoding.unwrap_or("").to_lowercase();
    // 可能有多重编码 "gzip, br"，取最后一个（最外层最后压缩的）
    let last = encoding
        .split(',')
        .last()
        .map(|s| s.trim())
        .unwrap_or("");

    match last {
        "gzip" => {
            let mut decoder = flate2::read::GzDecoder::new(bytes);
            let mut out = Vec::new();
            if std::io::Read::read_to_end(&mut decoder, &mut out).is_ok() {
                return out;
            }
            bytes.to_vec()
        }
        "deflate" => {
            let mut decoder = flate2::read::DeflateDecoder::new(bytes);
            let mut out = Vec::new();
            if std::io::Read::read_to_end(&mut decoder, &mut out).is_ok() {
                return out;
            }
            bytes.to_vec()
        }
        "br" => {
            let mut out = Vec::new();
            let mut decoder = brotli::Decompressor::new(bytes, 4096);
            if std::io::Read::read_to_end(&mut decoder, &mut out).is_ok() {
                return out;
            }
            bytes.to_vec()
        }
        _ => bytes.to_vec(),
    }
}

/// 判断响应体是否为文本类型（可安全展示），否则视为二进制。
pub fn is_text_content(content_type: Option<&str>) -> bool {
    let ct = content_type.unwrap_or("").to_lowercase();
    if ct.is_empty() {
        return true; // 无 content-type 默认按文本尝试
    }
    ct.contains("text")
        || ct.contains("json")
        || ct.contains("xml")
        || ct.contains("javascript")
        || ct.contains("x-www-form-urlencoded")
        || ct.contains("html")
        || ct.contains("form")
        || ct.contains("ndjson")
        || ct.contains("event-stream")
}
