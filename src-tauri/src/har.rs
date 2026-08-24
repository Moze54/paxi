//! HAR 1.2 导出：将存储中的抓包记录导出为 HTTP Archive 格式。
//!
//! 结构对齐 Chrome DevTools / Charles 的导出，可被 DevTools、Fiddler 等导入。
//! 二进制 body（占位符形式）导出为 text 并注明；文本 body 原样导出。

use crate::models::{RequestRecord, WsFrame};
use crate::storage::TrafficStore;
use serde_json::{json, Value};
use std::path::Path;

/// 导出全部记录为 HAR 文件，返回导出条数。
pub fn export_har(store: &dyn TrafficStore, path: &Path) -> Result<usize, String> {
    let metas = store.list();
    let mut entries = Vec::with_capacity(metas.len());

    for meta in metas {
        let Some(rec) = store.get(&meta.id) else {
            continue;
        };
        entries.push(record_to_entry(&rec));
    }

    let count = entries.len();
    let har = json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "paxi", "version": env!("CARGO_PKG_VERSION") },
            "entries": entries,
        }
    });

    let text = serde_json::to_string_pretty(&har).map_err(|e| format!("序列化 HAR 失败：{e}"))?;
    std::fs::write(path, text).map_err(|e| format!("写入 HAR 文件失败：{e}"))?;
    Ok(count)
}

/// 单条记录 → HAR entry。
fn record_to_entry(rec: &RequestRecord) -> Value {
    // startedDateTime: RFC3339 带毫秒
    let started = chrono::DateTime::from_timestamp_millis(rec.started_at as i64)
        .unwrap_or_else(chrono::Utc::now);
    let started_str = started.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // queryString 解析
    let query: Vec<Value> = parse_query(&rec.url)
        .into_iter()
        .map(|(k, v)| json!({ "name": k, "value": v }))
        .collect();

    // postData（请求体存在时）
    let post_data = rec.request_body.as_ref().map(|b| {
        json!({
            "mimeType": content_type_of(&rec.request_headers),
            "text": b,
        })
    });

    let mime = rec
        .content_type
        .clone()
        .unwrap_or_else(|| "text/plain".to_string());

    json!({
        "startedDateTime": started_str,
        "time": rec.duration_ms as u64,
        "request": {
            "method": rec.method,
            "url": rec.url,
            "httpVersion": "HTTP/1.1",
            "cookies": [],
            "headers": headers_to_json(&rec.request_headers),
            "queryString": query,
            "postData": post_data.unwrap_or(Value::Null),
            "headersSize": -1,
            "bodySize": rec.request_body_size as i64,
        },
        "response": {
            "status": rec.status,
            "statusText": status_text(rec.status),
            "httpVersion": "HTTP/1.1",
            "cookies": [],
            "headers": headers_to_json(&rec.response_headers),
            "content": {
                "size": rec.response_body_size as u64,
                "mimeType": mime,
                "text": rec.response_body.clone().unwrap_or_default(),
            },
            "redirectURL": header_value(&rec.response_headers, "location").unwrap_or_default(),
            "headersSize": -1,
            "bodySize": rec.response_body_size as i64,
        },
        "cache": {},
        "timings": {
            "send": 0,
            "wait": rec.duration_ms as u64,
            "receive": 0,
        },
        "_paxi": {
            "isReplay": rec.is_replay,
            "isWebsocket": rec.is_websocket,
            "wsFrameCount": rec.ws_frame_count,
            "matchedRule": rec.matched_rule,
            "clientIp": rec.client_ip,
            "error": rec.error,
        },
    })
}

fn headers_to_json(headers: &[(String, String)]) -> Vec<Value> {
    headers
        .iter()
        .map(|(k, v)| json!({ "name": k, "value": v }))
        .collect()
}

fn header_value(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == name)
        .map(|(_, v)| v.clone())
}

/// 请求体的 mimeType：优先请求头 content-type。
fn content_type_of(req_headers: &[(String, String)]) -> String {
    header_value(req_headers, "content-type")
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

/// 解析 URL query 参数（form-urlencoded）。
fn parse_query(url: &str) -> Vec<(String, String)> {
    let Some(q) = url.split_once('?').map(|(_, q)| q) else {
        return vec![];
    };
    // 去掉 fragment
    let q = q.split('#').next().unwrap_or(q);
    q.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

/// 轻量 percent-decoding（+ → 空格，%XX 解码）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 常见状态码文本。
fn status_text(status: u16) -> &'static str {
    match status {
        0 => "",
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// HAR 类型中未使用但保留的占位（避免 unused import 提示，未来 HAR 导入会用到）。
#[allow(dead_code)]
fn _ws_frame_placeholder(_f: &WsFrame) {}

// ==================== HAR 导入 ====================

/// 从 HAR 1.2 文件导入记录到存储，返回导入条数。
pub fn import_har(store: &dyn TrafficStore, path: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取 HAR 文件失败：{e}"))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("解析 HAR 失败：{e}"))?;

    let entries = json["log"]["entries"]
        .as_array()
        .ok_or("HAR 缺少 log.entries（不是合法的 HAR 1.2 文件）")?;

    let mut count = 0usize;
    for entry in entries {
        let rec = entry_to_record(entry);
        if let Some(rec) = rec {
            store.insert(rec);
            count += 1;
        }
    }
    Ok(count)
}

/// HAR entry → RequestRecord（尽力解析，缺失字段用默认值）。
fn entry_to_record(entry: &Value) -> Option<RequestRecord> {
    let req = entry.get("request")?;
    let url = req["url"].as_str()?.to_string();
    let method = req["method"].as_str().unwrap_or("GET").to_string();

    let uri = url.parse::<hyper::Uri>().ok()?;
    let host = uri.host().unwrap_or("unknown").to_string();
    let scheme = uri.scheme_str().unwrap_or("http").to_string();

    // 请求头
    let request_headers: Vec<(String, String)> = req["headers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    let name = h["name"].as_str()?;
                    let value = h["value"].as_str()?;
                    Some((name.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    // 请求体
    let req_body_text = req["postData"]["text"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    // 响应
    let resp = entry.get("response");
    let status = resp
        .and_then(|r| r["status"].as_u64())
        .map(|s| s as u16)
        .unwrap_or(0);
    let response_headers: Vec<(String, String)> = resp
        .and_then(|r| r["headers"].as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    let name = h["name"].as_str()?;
                    let value = h["value"].as_str()?;
                    Some((name.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let resp_body_text = resp
        .and_then(|r| r["content"]["text"].as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let resp_content_type = resp
        .and_then(|r| r["content"]["mimeType"].as_str())
        .map(|s| s.to_string());

    // 时间（RFC3339 → epoch ms）
    let started_at = entry["startedDateTime"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis() as u128)
        .unwrap_or_else(crate::models::now_ms);

    let request_body = if req_body_text.is_some() {
        req_body_text
    } else if request_headers
        .iter()
        .any(|(k, _)| k.to_lowercase() == "content-length")
    {
        Some("[导入 HAR：请求体已省略]".to_string())
    } else {
        None
    };

    Some(RequestRecord {
        id: uuid::Uuid::new_v4().to_string(),
        client_ip: None,
        client_process: None,
        method,
        url,
        host,
        scheme,
        status,
        request_headers,
        response_headers,
        request_body,
        response_body: resp_body_text,
        request_body_size: 0,
        response_body_size: 0,
        content_type: resp_content_type,
        duration_ms: entry["time"].as_u64().unwrap_or(0) as u128,
        started_at,
        error: None,
        is_websocket: false,
        ws_frame_count: 0,
        matched_rule: None,
        is_replay: false,
        passthrough: false,
    })
}
