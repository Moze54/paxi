//! paxi 核心引擎集成测试。
//!
//! 覆盖：
//! - 代理 e2e：本地上游 echo 服务器 + 明文 HTTP 经代理转发（真实 socket）
//! - 规则引擎：Mock / Redirect / Abort 命中与透传
//! - CA：域名 + IP 叶子证书签发（x509 SAN 校验）
//! - glob 匹配
//! - SQLite：读写回环 + 旧库迁移（自动补列）
//! - 重放：execute_replay 直发本地上游

use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use paxi_lib::ca::CertificateAuthority;
use paxi_lib::clients::ClientTracker;
use paxi_lib::events::EventHub;
use paxi_lib::models::RequestRecord;
use paxi_lib::proxy::ProxyEngine;
use paxi_lib::rules::{glob_match, Rule, RuleAction, RuleMatcher, RulesEngine};
use paxi_lib::storage::sqlite::SqliteStore;
use paxi_lib::storage::TrafficStore;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ==================== 辅助 ====================

/// 每个测试独立的临时目录（简单实现，测试结束不清理也不影响）。
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join("paxi-tests")
        .join(format!("{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 启动本地上游 echo HTTP 服务器，返回 (端口, 关闭信号)。
async fn start_upstream_echo() -> (u16, tokio::sync::oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        tokio::select! {
            _ = async {
                loop {
                    if let Ok((stream, _)) = listener.accept().await {
                        tokio::spawn(async move {
                            let service = service_fn(|req: Request<Incoming>| async move {
                                let (parts, body) = req.into_parts();
                                let b = body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                                let resp_body = format!(
                                    "echo {} {} body={}",
                                    parts.method,
                                    parts.uri.path(),
                                    String::from_utf8_lossy(&b)
                                );
                                Ok::<_, std::convert::Infallible>(
                                    Response::builder()
                                        .status(200)
                                        .header("content-type", "text/plain")
                                        .header("content-length", resp_body.len())
                                        .body(http_body_util::Full::<Bytes>::new(Bytes::from(resp_body)))
                                        .unwrap(),
                                )
                            });
                            let _ = http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service)
                                .await;
                        });
                    }
                }
            } => {}
            _ = rx => {}
        }
    });

    (port, tx)
}

/// 构建完整引擎（CA + SQLite + 规则 + 代理），返回 (engine, 代理端口, store, rules)。
async fn start_engine(tag: &str) -> (Arc<ProxyEngine>, u16, Arc<SqliteStore>, Arc<RulesEngine>) {
    let dir = temp_dir(tag);
    let ca = CertificateAuthority::load_or_create(&dir.join("ca")).unwrap();
    let store = SqliteStore::open(&dir.join("traffic.db"), &dir.join("bodies")).unwrap();
    let rules = RulesEngine::open(&dir.join("traffic.db")).unwrap();
    let engine = ProxyEngine::new(
        ca,
        store.clone(),
        EventHub::headless(),
        Arc::new(ClientTracker::headless()),
        rules.clone(),
    );

    // 选一个空闲端口（bind 0 探测后释放，重试 3 次避免竞态）
    let mut port = 0u16;
    for _ in 0..3 {
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let p = probe.local_addr().unwrap().port();
        drop(probe);
        match engine.start(p).await {
            Ok(_) => {
                port = p;
                break;
            }
            Err(_) => continue,
        }
    }
    assert_ne!(port, 0, "engine failed to start on any port");

    (engine, port, store, rules)
}

/// 裸 TCP 发送 HTTP 请求并读取完整响应（Connection: close 保证 EOF）。
async fn raw_http(port: u16, req_text: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(req_text.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).to_string()
}

/// 解析响应状态码与 body。
fn parse_resp(raw: &str) -> (u16, String) {
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

/// 轮询 store.list() 直到条件满足（SQLite 写线程是异步的）。
fn wait_for_list(store: &SqliteStore, pred: impl Fn(&[paxi_lib::models::RequestMeta]) -> bool) {
    for _ in 0..100 {
        if pred(&store.list()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("condition on store.list() not met within 2s");
}

/// 构造一条最小完整记录。
fn make_record(id: &str, url: &str, host: &str) -> RequestRecord {
    RequestRecord {
        id: id.to_string(),
        client_ip: Some("127.0.0.1".into()),
        client_process: None,
        method: "GET".into(),
        url: url.into(),
        host: host.into(),
        scheme: "https".into(),
        status: 200,
        request_headers: vec![("accept".into(), "*/*".into())],
        response_headers: vec![("content-type".into(), "text/plain".into())],
        request_body: Some("req".into()),
        response_body: Some("resp".into()),
        request_body_size: 3,
        response_body_size: 4,
        content_type: Some("text/plain".into()),
        duration_ms: 12,
        started_at: 1_700_000_000_000,
        error: None,
        is_websocket: false,
        ws_frame_count: 0,
        matched_rule: None,
        is_replay: false,
        passthrough: false,
    }
}

// ==================== e2e：代理转发 ====================

#[tokio::test]
async fn proxy_forwards_plain_http_and_records() {
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (_engine, port, store, _rules) = start_engine("e2e-forward").await;

    let raw = raw_http(
        port,
        &format!(
            "GET http://127.0.0.1:{up_port}/hello?x=1 HTTP/1.1\r\n\
             Host: 127.0.0.1:{up_port}\r\n\
             Connection: close\r\n\r\n"
        ),
    )
    .await;
    let (status, body) = parse_resp(&raw);
    assert_eq!(status, 200, "raw: {raw}");
    assert!(body.contains("echo GET /hello"), "body: {body}");

    // 记录已入库
    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/hello")));
    let list = store.list();
    let meta = list.iter().find(|m| m.url.contains("/hello")).unwrap();
    assert_eq!(meta.status, 200);
    assert_eq!(meta.host, "127.0.0.1"); // Uri::host() 不含端口
}

#[tokio::test]
async fn proxy_forwards_post_body() {
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (_engine, port, store, _rules) = start_engine("e2e-post").await;

    let body = "name=paxi";
    let raw = raw_http(
        port,
        &format!(
            "POST http://127.0.0.1:{up_port}/submit HTTP/1.1\r\n\
             Host: 127.0.0.1:{up_port}\r\n\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        ),
    )
    .await;
    let (status, resp_body) = parse_resp(&raw);
    assert_eq!(status, 200);
    assert!(resp_body.contains("echo POST /submit"), "{resp_body}");
    assert!(resp_body.contains("body=name=paxi"), "{resp_body}");

    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/submit")));
}

// ==================== e2e：规则引擎 ====================

#[tokio::test]
async fn rule_mock_returns_constructed_response() {
    let (_engine, port, store, rules) = start_engine("rule-mock").await;
    rules
        .upsert(Rule {
            id: "r-mock".into(),
            name: "mock-api".into(),
            enabled: true,
            priority: 10,
            matcher: RuleMatcher {
                host: Some("api.test.com".into()),
                path: Some("/v1/*".into()),
                method: None,
            },
            action: RuleAction::Mock {
                status: 201,
                content_type: "application/json".into(),
                body: r#"{"ok":true}"#.into(),
            },
            hits: 0,
            updated_at: 0,
        })
        .unwrap();

    let raw = raw_http(
        port,
        "GET http://api.test.com/v1/user HTTP/1.1\r\n\
         Host: api.test.com\r\n\
         Connection: close\r\n\r\n",
    )
    .await;
    let (status, body) = parse_resp(&raw);
    assert_eq!(status, 201, "raw: {raw}");
    assert!(body.contains(r#""ok":true"#), "{body}");
    assert!(raw.contains("x-paxi-rule: mock-api"), "raw: {raw}");

    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/v1/user")));
    let list = store.list();
    let meta = list.iter().find(|m| m.url.contains("/v1/user")).unwrap();
    assert_eq!(meta.status, 201);
    // matched_rule 在详情记录上断言
    let rec = store.get(&meta.id).unwrap();
    assert_eq!(rec.matched_rule.as_deref(), Some("mock-api"));
}

#[tokio::test]
async fn rule_redirect_and_abort() {
    let (_engine, port, store, rules) = start_engine("rule-redir-abort").await;
    rules
        .upsert(Rule {
            id: "r-redir".into(),
            name: "redir".into(),
            enabled: true,
            priority: 10,
            matcher: RuleMatcher {
                host: Some("old.test.com".into()),
                path: None,
                method: None,
            },
            action: RuleAction::Redirect {
                to: "https://new.test.com/".into(),
                status: 302,
            },
            hits: 0,
            updated_at: 0,
        })
        .unwrap();
    rules
        .upsert(Rule {
            id: "r-abort".into(),
            name: "block-ads".into(),
            enabled: true,
            priority: 20,
            matcher: RuleMatcher {
                host: Some("ads.test.com".into()),
                path: None,
                method: None,
            },
            action: RuleAction::Abort,
            hits: 0,
            updated_at: 0,
        })
        .unwrap();

    // Redirect
    let raw = raw_http(
        port,
        "GET http://old.test.com/a HTTP/1.1\r\nHost: old.test.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (status, _) = parse_resp(&raw);
    assert_eq!(status, 302, "{raw}");
    assert!(raw.contains("location: https://new.test.com/"), "{raw}");

    // Abort
    let raw = raw_http(
        port,
        "GET http://ads.test.com/x.png HTTP/1.1\r\nHost: ads.test.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (status, body) = parse_resp(&raw);
    assert_eq!(status, 403, "{raw}");
    assert!(body.contains("Blocked"), "{body}");

    wait_for_list(&store, |l| l.len() >= 2);
}

#[tokio::test]
async fn rule_disabled_does_not_match() {
    let (_engine, port, _store, rules) = start_engine("rule-disabled").await;
    rules
        .upsert(Rule {
            id: "r-off".into(),
            name: "off".into(),
            enabled: false,
            priority: 10,
            matcher: RuleMatcher {
                host: Some("api.test.com".into()),
                path: None,
                method: None,
            },
            action: RuleAction::Mock {
                status: 599,
                content_type: "text/plain".into(),
                body: "should not appear".into(),
            },
            hits: 0,
            updated_at: 0,
        })
        .unwrap();

    // 上游连不上（api.test.com 不可达），但不应是 599 mock
    let raw = raw_http(
        port,
        "GET http://api.test.com/anything HTTP/1.1\r\nHost: api.test.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (status, _) = parse_resp(&raw);
    assert_ne!(status, 599, "disabled rule should not fire: {raw}");
}

// ==================== e2e：重放 ====================

#[tokio::test]
async fn replay_hits_upstream_directly() {
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (engine, _port, store, _rules) = start_engine("replay").await;

    let meta = engine
        .execute_replay(paxi_lib::proxy::ReplayParams {
            method: "POST".into(),
            url: format!("http://127.0.0.1:{up_port}/replay-target"),
            headers: vec![("content-type".into(), "text/plain".into())],
            body: Some("hello-replay".into()),
        })
        .await
        .unwrap();

    assert_eq!(meta.status, 200);
    assert!(meta.is_replay);

    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/replay-target")));
    let list = store.list();
    let meta = list.iter().find(|m| m.url.contains("/replay-target")).unwrap();
    let rec = store.get(&meta.id).unwrap();
    assert!(rec.is_replay);
    assert!(rec.response_body.as_deref().unwrap_or("").contains("echo POST /replay-target"));
}

// ==================== e2e：断点调试 ====================

#[tokio::test]
async fn breakpoint_pauses_then_forwards_edited() {
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (engine, port, store, rules) = start_engine("bp-forward").await;
    rules
        .upsert(Rule {
            id: "bp".into(),
            name: "bp-api".into(),
            enabled: true,
            priority: 10,
            matcher: RuleMatcher {
                host: Some("127.0.0.1".into()),
                path: Some("/bp/*".into()),
                method: None,
            },
            action: RuleAction::Breakpoint,
            hits: 0,
            updated_at: 0,
        })
        .unwrap();

    // 发起请求（挂起中，不返回）
    let port_clone = port;
    let req_task = tokio::spawn(async move {
        raw_http(
            port_clone,
            &format!(
                "GET http://127.0.0.1:{up_port}/bp/hello HTTP/1.1\r\n\
                 Host: 127.0.0.1:{up_port}\r\n\
                 Connection: close\r\n\r\n"
            ),
        )
        .await
    });

    // 等待断点挂起
    let mut info = None;
    for _ in 0..50 {
        let list = engine.list_breakpoints();
        if !list.is_empty() {
            info = Some(list[0].clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let info = info.expect("breakpoint should be pending");
    assert!(info.url.contains("/bp/hello"));
    assert_eq!(info.method, "GET");

    // 放行（把路径改成 /bp/edited）
    let edited_url = format!("http://127.0.0.1:{up_port}/bp/edited");
    engine
        .resume_breakpoint(
            &info.bp_id,
            paxi_lib::proxy::BreakpointDecision::Forward {
                method: "GET".into(),
                url: edited_url,
                headers: vec![],
                body: None,
            },
        )
        .unwrap();

    let raw = req_task.await.unwrap();
    let (status, body) = parse_resp(&raw);
    assert_eq!(status, 200, "{raw}");
    assert!(body.contains("/bp/edited"), "forwarded URL should be edited: {body}");

    // 断点已清空
    assert!(engine.list_breakpoints().is_empty());
    // 记录已入库
    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/bp/edited")));
}

#[tokio::test]
async fn breakpoint_abort_returns_403() {
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (engine, port, store, rules) = start_engine("bp-abort").await;
    rules
        .upsert(Rule {
            id: "bp2".into(),
            name: "bp-abort".into(),
            enabled: true,
            priority: 10,
            matcher: RuleMatcher {
                host: Some("127.0.0.1".into()),
                path: Some("/block/*".into()),
                method: None,
            },
            action: RuleAction::Breakpoint,
            hits: 0,
            updated_at: 0,
        })
        .unwrap();

    let port_clone = port;
    let req_task = tokio::spawn(async move {
        raw_http(
            port_clone,
            &format!(
                "GET http://127.0.0.1:{up_port}/block/x HTTP/1.1\r\n\
                 Host: 127.0.0.1:{up_port}\r\n\
                 Connection: close\r\n\r\n"
            ),
        )
        .await
    });

    let mut bp_id = None;
    for _ in 0..50 {
        let list = engine.list_breakpoints();
        if !list.is_empty() {
            bp_id = Some(list[0].bp_id.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let bp_id = bp_id.expect("breakpoint should be pending");
    let _ = engine
        .resume_breakpoint(&bp_id, paxi_lib::proxy::BreakpointDecision::Abort)
        .unwrap();

    let raw = req_task.await.unwrap();
    let (status, body) = parse_resp(&raw);
    assert_eq!(status, 403, "{raw}");
    assert!(body.contains("Blocked"), "{body}");
    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/block/x")));
}

// ==================== e2e：弱网模拟（首字节延迟） ====================

#[tokio::test]
async fn throttle_delay_applies_before_forward() {
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (_engine, port, store, rules) = start_engine("throttle").await;
    rules
        .upsert(Rule {
            id: "th".into(),
            name: "slow".into(),
            enabled: true,
            priority: 10,
            matcher: RuleMatcher {
                host: Some("127.0.0.1".into()),
                path: None,
                method: None,
            },
            action: RuleAction::Throttle {
                kbps: 0,
                delay_ms: 300,
                drop_pct: 0,
            },
            hits: 0,
            updated_at: 0,
        })
        .unwrap();

    let started = std::time::Instant::now();
    let raw = raw_http(
        port,
        &format!(
            "GET http://127.0.0.1:{up_port}/slow HTTP/1.1\r\n\
             Host: 127.0.0.1:{up_port}\r\n\
             Connection: close\r\n\r\n"
        ),
    )
    .await;
    let elapsed = started.elapsed().as_millis();
    let (status, body) = parse_resp(&raw);
    assert_eq!(status, 200, "{raw}");
    assert!(body.contains("echo GET /slow"), "{body}");
    assert!(elapsed >= 250, "throttle delay should slow down request, took {elapsed}ms");

    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/slow")));
}

// ==================== 单元：HAR 导入 ====================

#[test]
fn har_import_roundtrip() {
    let dir = temp_dir("har-import");
    let store = SqliteStore::open(&dir.join("t.db"), &dir.join("bodies")).unwrap();

    let har_path = dir.join("sample.har");
    std::fs::write(
        &har_path,
        r#"{
  "log": {
    "version": "1.2",
    "creator": { "name": "unit-test", "version": "1" },
    "entries": [
      {
        "startedDateTime": "2024-01-01T10:00:00.000Z",
        "time": 123,
        "request": {
          "method": "POST",
          "url": "https://api.sample.com/v1/upload?lang=zh",
          "headers": [
            { "name": "content-type", "value": "application/json" },
            { "name": "authorization", "value": "Bearer abc123" }
          ],
          "postData": { "mimeType": "application/json", "text": "{\"a\":1}" }
        },
        "response": {
          "status": 201,
          "statusText": "Created",
          "headers": [
            { "name": "content-type", "value": "application/json" }
          ],
          "content": { "mimeType": "application/json", "text": "{\"ok\":true}" }
        }
      }
    ]
  }
}"#,
    )
    .unwrap();

    let count = paxi_lib::har::import_har(store.as_ref(), &har_path).unwrap();
    assert_eq!(count, 1);

    wait_for_list(&store, |l| !l.is_empty());
    let list = store.list();
    assert_eq!(list[0].method, "POST");
    assert_eq!(list[0].status, 201);
    assert!(list[0].url.contains("/v1/upload"));
    assert_eq!(list[0].duration_ms, 123);

    let rec = store.get(&list[0].id).unwrap();
    assert_eq!(rec.request_body.as_deref(), Some(r#"{"a":1}"#));
    assert_eq!(rec.response_body.as_deref(), Some(r#"{"ok":true}"#));
    assert!(rec.request_headers.iter().any(|(k, _)| k == "authorization"));
}

// ==================== e2e：上游代理链 ====================

#[tokio::test]
async fn upstream_proxy_forwards_via_second_hop() {
    // 第一个"上游代理"（实际是处理绝对 URL 的 HTTP 服务器）
    let (up_port, _up_tx) = start_upstream_echo().await;
    let (engine, port, store, _rules) = start_engine("upstream").await;

    // 配置引擎使用该上游代理
    engine.set_upstream(Some(paxi_lib::proxy::UpstreamProxy {
        enabled: true,
        host: "127.0.0.1".into(),
        port: up_port,
        username: String::new(),
        password: String::new(),
    }));

    let raw = raw_http(
        port,
        &format!(
            "GET http://target.example.com/api/items?x=1 HTTP/1.1\r\n\
             Host: target.example.com\r\n\
             Connection: close\r\n\r\n"
        ),
    )
    .await;
    let (status, body) = parse_resp(&raw);
    // 上游 echo 把路径回显，证明请求确实经上游代理转发
    assert_eq!(status, 200, "{raw}");
    assert!(body.contains("echo GET /api/items"), "via upstream: {body}");

    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/api/items")));
    let list = store.list();
    let meta = list.iter().find(|m| m.url.contains("/api/items")).unwrap();
    assert_eq!(meta.status, 200);

    // 关闭上游代理
    engine.set_upstream(None);
}

// ==================== 单元：统计聚合 ====================

#[test]
fn stats_compute_aggregates() {
    let dir = temp_dir("stats");
    let store = SqliteStore::open(&dir.join("t.db"), &dir.join("bodies")).unwrap();

    let mut r1 = make_record("s1", "https://a.com/1", "a.com");
    r1.status = 200;
    r1.method = "GET".into();
    r1.duration_ms = 100;
    r1.scheme = "https".into();
    let mut r2 = make_record("s2", "https://a.com/2", "a.com");
    r2.status = 404;
    r2.method = "POST".into();
    r2.scheme = "https".into();
    let mut r3 = make_record("s3", "ws://b.com/sock", "b.com");
    r3.status = 101;
    r3.is_websocket = true;
    r3.method = "GET".into();
    r3.scheme = "wss".into();
    let mut r4 = make_record("s4", "http://c.com/x", "c.com");
    r4.status = 0;
    r4.method = "GET".into();
    r4.scheme = "http".into();

    store.insert(r1);
    store.insert(r2);
    store.insert(r3);
    store.insert(r4);
    wait_for_list(&store, |l| l.len() == 4);

    let stats = paxi_lib::stats::compute(store.as_ref());
    assert_eq!(stats.total, 4);
    assert_eq!(stats.succeeded, 2); // 200 + 101
    assert_eq!(stats.failed, 2); // 404 + 0
    // 状态码分布含 200/404/101/error
    let keys: Vec<String> = stats.status_dist.iter().map(|(k, _)| k.clone()).collect();
    assert!(keys.contains(&"200".to_string()));
    assert!(keys.contains(&"404".to_string()));
    assert!(keys.contains(&"error".to_string()));
    // 方法分布：GET×3 POST×1
    let get = stats.method_dist.iter().find(|(k, _)| k == "GET").unwrap();
    assert_eq!(get.1, 3);
    // 协议分布：https×2 ws×1 http×1
    let ws = stats.scheme_dist.iter().find(|(k, _)| k == "ws").unwrap();
    assert_eq!(ws.1, 1);
    // 平均耗时 (100+12*3)/4 = 34
    assert_eq!(stats.avg_duration_ms, 34);
    // 域名 TOP：a.com ×2 居首
    assert_eq!(stats.host_top[0].0, "a.com");
    assert_eq!(stats.host_top[0].1, 2);
    // 时间线 24 桶
    assert_eq!(stats.timeline.len(), 24);
}

// ==================== e2e：来源进程识别 ====================

#[tokio::test]
async fn source_process_detected_for_local_connection() {
    let (_engine, port, store, _rules) = start_engine("source-process").await;

    // 本测试进程发起请求（真实 TCP 连接）
    let raw = raw_http(
        port,
        "GET http://example.test/hello-proc HTTP/1.1\r\n\
         Host: example.test\r\n\
         Connection: close\r\n\r\n",
    )
    .await;
    let (status, _) = parse_resp(&raw);
    // 上游不可达也算记录（转发失败），列表应有记录
    let _ = status;

    wait_for_list(&store, |l| l.iter().any(|m| m.url.contains("/hello-proc")));
    let list = store.list();
    let meta = list.iter().find(|m| m.url.contains("/hello-proc")).unwrap();
    let rec = store.get(&meta.id).unwrap();

    // 本机连接必须解析出进程名（engine_test.exe）
    assert!(
        rec.client_process.is_some(),
        "client_process 应为 Some（ip={:?}）",
        rec.client_ip
    );
    let proc = rec.client_process.as_deref().unwrap();
    assert!(
        proc.to_lowercase().contains("engine_test") || proc.to_lowercase().contains("paxi"),
        "进程名异常：{proc}"
    );
    assert_eq!(rec.client_ip.as_deref(), Some("127.0.0.1"));
}

// ==================== 单元：glob 匹配 ====================

#[test]
fn glob_matching_cases() {
    assert!(glob_match("*.example.com", "api.example.com"));
    // 大小写不敏感
    assert!(glob_match("*.example.com", "API.Example.COM"));
    // 裸域不带子域：* 需匹配空前缀，".example.com" != "example.com"，不命中
    assert!(!glob_match("*.example.com", "example.com"));
    assert!(!glob_match("*.example.com", "example.com.evil.net"));
    assert!(glob_match("/api/*", "/api/v1/users"));
    assert!(glob_match("/api/*", "/api/"));
    assert!(!glob_match("/api/*", "/web/x"));
    assert!(glob_match("a?c", "abc"));
    assert!(!glob_match("a?c", "abbc"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("exact.host", "exact.host"));
    assert!(!glob_match("exact.host", "other.host"));
    assert!(glob_match("192.168.*", "192.168.1.5"));
    // 多重星号
    assert!(glob_match("*a*b*", "xxaxxbxx"));
    assert!(!glob_match("*a*b*", "xxbaxx"));
}

// ==================== 单元：CA 叶子证书 ====================

#[test]
fn ca_signs_leaf_for_domain_and_ip() {
    let dir = temp_dir("ca-leaf");
    let ca = CertificateAuthority::load_or_create(&dir).unwrap();

    // 域名叶子
    let (cert_pem, key_pem) = ca.leaf_for_host("www.example.com").unwrap();
    assert!(cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(key_pem.contains("BEGIN PRIVATE KEY") || key_pem.contains("BEGIN RSA PRIVATE KEY"));

    // 缓存命中：同域名返回相同证书
    let (cert2, _) = ca.leaf_for_host("www.example.com").unwrap();
    assert_eq!(cert_pem, cert2);

    // IP 叶子：能签发 + SAN 是 IP 类型
    let (ip_cert_pem, _) = ca.leaf_for_host("192.168.1.5").unwrap();
    let der = rustls_pemfile::certs(&mut ip_cert_pem.as_bytes())
        .next()
        .unwrap()
        .unwrap();
    let (_, parsed) = x509_parser::parse_x509_certificate(&der).unwrap();
    let san_str = format!("{:?}", parsed.extensions());
    // x509-parser 的 Debug 打印 IP SAN 为字节序列或点分文本，两种都接受
    assert!(
        san_str.contains("192.168.1.5") || san_str.contains("[192, 168, 1, 5]"),
        "IP SAN missing in cert extensions: {san_str}"
    );
}

// ==================== 单元：SQLite 回环与迁移 ====================

#[test]
fn sqlite_roundtrip() {
    let dir = temp_dir("sqlite-rt");
    let store = SqliteStore::open(&dir.join("t.db"), &dir.join("bodies")).unwrap();

    store.insert(make_record("rt-1", "https://a.com/x", "a.com"));
    wait_for_list(&store, |l| !l.is_empty());

    let meta = &store.list()[0];
    assert_eq!(meta.host, "a.com");
    assert_eq!(meta.status, 200);

    let rec = store.get("rt-1").unwrap();
    assert_eq!(rec.method, "GET");
    assert_eq!(rec.response_body.as_deref(), Some("resp"));
    assert!(!rec.is_replay);
    assert!(!rec.passthrough);

    // 清空
    store.clear();
    wait_for_list(&store, |l| l.is_empty());
}

#[test]
fn sqlite_migrates_old_schema() {
    let dir = temp_dir("sqlite-mig");
    let db = dir.join("old.db");

    // 模拟 M1 老库：无 matched_rule / is_replay / passthrough 列
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE record (
                id TEXT PRIMARY KEY, client_ip TEXT, method TEXT, url TEXT, host TEXT,
                scheme TEXT, status INTEGER, req_headers TEXT, resp_headers TEXT,
                req_body TEXT, resp_body TEXT, req_body_file TEXT, resp_body_file TEXT,
                req_size INTEGER, resp_size INTEGER, content_type TEXT,
                duration_ms INTEGER, started_at INTEGER, error TEXT,
                is_websocket INTEGER, ws_frame_count INTEGER
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO record VALUES ('old-1','127.0.0.1','GET','https://old.com/','old.com',
             'https',200,'[]','[]',NULL,NULL,NULL,NULL,0,0,NULL,5,1700000000000,NULL,0,0)",
            [],
        )
        .unwrap();
    }

    // 打开：自动补列，老数据可读
    let store = SqliteStore::open(&db, &dir.join("bodies")).unwrap();
    let list = store.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "old-1");
    assert!(!list[0].is_replay);
    assert!(!list[0].passthrough);

    let rec = store.get("old-1").unwrap();
    assert!(rec.matched_rule.is_none());

    // 迁移后可继续写入新格式记录
    store.insert(make_record("new-1", "https://new.com/y", "new.com"));
    wait_for_list(&store, |l| l.len() == 2);
    assert!(store.get("new-1").unwrap().is_replay == false);
}
