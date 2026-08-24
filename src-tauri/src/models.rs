//! 流量记录的数据结构与内存存储。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 单条抓包记录的完整数据结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    /// 唯一 id
    pub id: String,
    /// 请求方法，如 GET / POST
    pub method: String,
    /// 完整 URL
    pub url: String,
    /// 主机名
    pub host: String,
    /// 协议 http / https / ws / wss
    pub scheme: String,
    /// 请求体（正文），可能被截断
    pub request_body: Option<String>,
    /// 响应体（正文），可能被截断
    pub response_body: Option<String>,
    /// 状态码
    pub status: u16,
    /// 请求头
    pub request_headers: Vec<(String, String)>,
    /// 响应头
    pub response_headers: Vec<(String, String)>,
    /// 耗时（毫秒）
    pub duration_ms: u128,
    /// 开始时间（epoch 毫秒）
    pub started_at: u128,
    /// 错误信息（若有）
    pub error: Option<String>,
    /// 是否为 WebSocket 握手
    pub is_websocket: bool,
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
    pub error: Option<String>,
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
            error: r.error.clone(),
        }
    }
}

/// 内存流量存储（环形缓冲）。
pub struct TrafficStore {
    /// 完整记录映射 id -> record
    records: Mutex<HashMap<String, RequestRecord>>,
    /// 有序的 id 列表（新->旧，用于列表展示）
    order: Mutex<Vec<String>>,
    /// 最大保留条数
    capacity: usize,
}

impl TrafficStore {
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            records: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            capacity,
        })
    }

    /// 插入一条记录，返回 meta。
    pub fn insert(self: &Arc<Self>, record: RequestRecord) -> RequestMeta {
        let meta = RequestMeta::from(&record);
        let id = record.id.clone();
        {
            let mut order = self.order.lock().unwrap();
            order.insert(0, id.clone());
            // 环形缓冲：超出容量时移除最旧的
            while order.len() > self.capacity {
                if let Some(old) = order.pop() {
                    self.records.lock().unwrap().remove(&old);
                }
            }
        }
        self.records.lock().unwrap().insert(id, record);
        meta
    }

    /// 获取列表（最新在前）。
    pub fn list(&self) -> Vec<RequestMeta> {
        let order = self.order.lock().unwrap();
        let records = self.records.lock().unwrap();
        order
            .iter()
            .filter_map(|id| records.get(id).map(RequestMeta::from))
            .collect()
    }

    /// 获取单条详情。
    pub fn get(&self, id: &str) -> Option<RequestRecord> {
        self.records.lock().unwrap().get(id).cloned()
    }

    /// 清空所有记录。
    pub fn clear(&self) {
        self.records.lock().unwrap().clear();
        self.order.lock().unwrap().clear();
    }
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
}
