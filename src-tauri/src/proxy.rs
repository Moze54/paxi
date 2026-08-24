//! HTTP/HTTPS/WebSocket 中间人代理引擎。
//!
//! - 监听 0.0.0.0:port（默认 8888）
//! - 目标是代理自身的请求 → 门户服务（证书下载/引导页），修复回环 bug
//! - 普通 HTTP 请求：转发到真实服务器，记录请求/响应
//! - HTTPS CONNECT：与客户端用动态域名证书建 TLS，解密后按 HTTP 转发
//! - WebSocket：完成 101 握手后双向转发并逐帧记录
//! - TLS 直通：命中直通列表的 host 不做 MITM，直接隧道转发（应对 SSL Pinning）
//!
//! CONNECT 中间人流程：
//! 1. 收到 `CONNECT host:443`，返回 200 并拿到升级流
//! 2. 用 CA 为 host 签发叶子证书，在升级流上做 TLS 服务端握手
//! 3. 握手成功后，客户端发送真实的 HTTP（HTTPS）请求，此时走普通 HTTP 转发逻辑

use crate::ca::CertificateAuthority;
use crate::clients::ClientTracker;
use crate::events::EventHub;
use crate::models::{body_to_string, decode_body, is_text_content, now_ms, RequestMeta, RequestRecord};
use crate::portal::{self, Portal};
use crate::rules::{RuleAction, RulesEngine};
use crate::storage::TrafficStore;
use crate::ws;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde::Serialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use uuid::Uuid;

/// 安装 rustls CryptoProvider（ring），确保 TLS 可用。
static INSTALL_RUSTLS_PROVIDER: std::sync::Once = std::sync::Once::new();

fn ensure_rustls_provider() {
    INSTALL_RUSTLS_PROVIDER.call_once(|| {
        // 若已被安装则忽略错误；否则安装 ring provider
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    });
}

/// 全局代理状态。
#[derive(Clone, Serialize)]
pub struct ProxyState {
    pub running: bool,
    pub port: u16,
    pub local_ip: String,
}

/// CONNECT 升级上下文：承载 upgrade future 及后续 TLS 中间人所需的信息。
struct UpgradeContext {
    on_upgrade: hyper::upgrade::OnUpgrade,
    host: String,
    ca: Arc<CertificateAuthority>,
    engine: Arc<ProxyEngine>,
    client_ip: String,
    client_app: Option<String>,
}

/// 代理引擎。
pub struct ProxyEngine {
    ca: Arc<CertificateAuthority>,
    store: Arc<dyn TrafficStore>,
    hub: EventHub,
    clients: Arc<ClientTracker>,
    /// 规则引擎（Mock / Redirect / Delay / Abort）。
    rules: Arc<RulesEngine>,
    /// 门户服务（启动时构建，含最新本机 IP）。
    portal: Mutex<Option<Arc<Portal>>>,
    /// 用于向真实服务器发请求的 HTTP 客户端（支持 HTTPS，上游启用 h2）。
    client: Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>,
    /// 当前 TCP 监听器（用于停止）。
    listener: Mutex<Option<Arc<TcpListener>>>,
    state: Mutex<ProxyState>,
    /// TLS 直通域名列表（glob，命中则不做 MITM，直接隧道转发）。
    passthrough_hosts: Mutex<Vec<String>>,
    /// 挂起中的断点：bp_id -> 待决策句柄。
    breakpoints: Mutex<std::collections::HashMap<String, BreakpointPending>>,
    /// 上游代理配置（公司代理场景）。
    upstream: Mutex<Option<UpstreamProxy>>,
}

/// 上游代理配置（HTTP 代理）。
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct UpstreamProxy {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

/// 挂起中的断点（内部句柄）。
struct BreakpointPending {
    info: BreakpointInfo,
    tx: tokio::sync::oneshot::Sender<BreakpointDecision>,
}

/// 断点快照（推送给前端展示/编辑）。
#[derive(Clone, Serialize)]
pub struct BreakpointInfo {
    pub bp_id: String,
    pub record_id: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub started_at: u128,
}

/// 断点决策（前端 resume_breakpoint 传入）。
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BreakpointDecision {
    /// 放行（可携带修改后的请求）
    Forward {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<String>,
    },
    /// 拦截（返回 403）
    Abort,
}

impl ProxyEngine {
    pub fn new(
        ca: Arc<CertificateAuthority>,
        store: Arc<dyn TrafficStore>,
        hub: EventHub,
        clients: Arc<ClientTracker>,
        rules: Arc<RulesEngine>,
    ) -> Arc<Self> {
        ensure_rustls_provider();
        // 构建支持 HTTPS 的客户端；启用上游 HTTP/2，兼容仅支持 h2 的站点
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("failed to load native root certs")
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        Arc::new(Self {
            ca,
            store,
            hub,
            clients,
            rules,
            portal: Mutex::new(None),
            client: Client::builder(TokioExecutor::new()).build(https),
            listener: Mutex::new(None),
            state: Mutex::new(ProxyState {
                running: false,
                port: 8888,
                local_ip: String::new(),
            }),
            passthrough_hosts: Mutex::new(Vec::new()),
            breakpoints: Mutex::new(std::collections::HashMap::new()),
            upstream: Mutex::new(None),
        })
    }

    /// 启动代理，监听 0.0.0.0:port。
    pub async fn start(self: &Arc<Self>, port: u16) -> Result<ProxyState, String> {
        self.stop().await;

        let addr: SocketAddr = format!("0.0.0.0:{port}")
            .parse::<SocketAddr>()
            .map_err(|e: std::net::AddrParseError| e.to_string())?;

        // 绑定，若被占用则等待后重试一次（处理 TIME_WAIT / 快速重启竞态）
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[proxy] port {port} first bind failed ({e}), retrying...");
                tokio::time::sleep(Duration::from_millis(500)).await;
                TcpListener::bind(&addr)
                    .await
                    .map_err(|e| format!("绑定端口 {port} 失败：{e}"))?
            }
        };
        let listener = Arc::new(listener);

        let local_ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        // 构建门户（含本机全部 IP，用于自引用识别）
        {
            let portal = Portal::new(self.ca.root_cert_pem(), port, local_ip.clone());
            *self.portal.lock().unwrap() = Some(portal);
        }

        {
            let mut state = self.state.lock().unwrap();
            state.running = true;
            state.port = port;
            state.local_ip = local_ip.clone();
        }

        let engine = self.clone();
        let listener_clone = listener.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match listener_clone.accept().await {
                    Ok((stream, peer)) => {
                        // 客户端感知
                        engine.clients.track(peer.ip());
                        let engine = engine.clone();
                        let client_ip = peer.ip().to_string();
                        let client_port = peer.port();
                        // 来源进程识别（Windows；手机等远程设备为 None）
                        let current_port = engine.status().port;
                        let client_app = crate::process::resolve_app(current_port, &client_ip, client_port);
                        tauri::async_runtime::spawn(async move {
                            let _ = engine.handle_connection(stream, client_ip, client_app).await;
                        });
                    }
                    Err(_) => break, // 监听器关闭，退出循环
                }
            }
        });

        *self.listener.lock().unwrap() = Some(listener);
        Ok(self.state.lock().unwrap().clone())
    }

    /// 停止代理。
    pub async fn stop(&self) {
        let listener = self.listener.lock().unwrap().take();
        drop(listener); // 关闭监听 socket，让 accept 循环退出
        let mut state = self.state.lock().unwrap();
        state.running = false;
    }

    /// 获取当前状态。
    pub fn status(&self) -> ProxyState {
        self.state.lock().unwrap().clone()
    }

    /// 设置 TLS 直通域名列表（glob 通配，如 `*.wechat.com`）。
    pub fn set_passthrough_hosts(&self, hosts: Vec<String>) {
        *self.passthrough_hosts.lock().unwrap() = hosts;
    }

    /// 当前直通列表。
    pub fn passthrough_hosts(&self) -> Vec<String> {
        self.passthrough_hosts.lock().unwrap().clone()
    }

    /// host 是否命中直通列表。
    fn is_passthrough(&self, host: &str) -> bool {
        let hosts = self.passthrough_hosts.lock().unwrap();
        hosts
            .iter()
            .any(|p| crate::rules::glob_match(p.trim(), host))
    }

    /// 设置上游代理。None 清除。
    pub fn set_upstream(&self, u: Option<UpstreamProxy>) {
        *self.upstream.lock().unwrap() = u;
    }

    /// 当前上游代理配置。
    pub fn upstream(&self) -> Option<UpstreamProxy> {
        self.upstream.lock().unwrap().clone()
    }

    /// 是否启用了上游代理。
    fn upstream_enabled(&self) -> bool {
        self.upstream
            .lock()
            .unwrap()
            .as_ref()
            .map(|u| u.enabled && !u.host.is_empty())
            .unwrap_or(false)
    }

    // ===== 断点调试 =====

    /// 挂起请求等待前端决策；超时（5 分钟）或通道关闭返回 None（原样放行）。
    #[allow(clippy::too_many_arguments)]
    async fn wait_breakpoint(
        self: &Arc<Self>,
        record_id: &str,
        method: &str,
        url: &str,
        headers: Vec<(String, String)>,
        body: Option<String>,
        started_at: u128,
    ) -> Option<BreakpointDecision> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let bp_id = Uuid::new_v4().to_string();
        let info = BreakpointInfo {
            bp_id: bp_id.clone(),
            record_id: record_id.to_string(),
            method: method.to_string(),
            url: url.to_string(),
            headers,
            body,
            started_at,
        };
        self.breakpoints
            .lock()
            .unwrap()
            .insert(bp_id.clone(), BreakpointPending { info: info.clone(), tx });
        // 低频事件：立即推送，不进合并窗口
        self.hub.push_breakpoint(info);

        let decision = tokio::time::timeout(Duration::from_secs(300), rx).await;
        // 无论结果如何都清理挂起表
        self.breakpoints.lock().unwrap().remove(&bp_id);
        match decision {
            Ok(Ok(d)) => Some(d),
            _ => None, // 超时或发送端被 drop（不应发生）
        }
    }

    /// 当前挂起的断点列表（前端轮询/刷新用）。
    pub fn list_breakpoints(&self) -> Vec<BreakpointInfo> {
        self.breakpoints
            .lock()
            .unwrap()
            .values()
            .map(|p| p.info.clone())
            .collect()
    }

    /// 恢复断点：应用前端决策。
    pub fn resume_breakpoint(
        &self,
        bp_id: &str,
        decision: BreakpointDecision,
    ) -> Result<(), String> {
        let pending = self.breakpoints.lock().unwrap().remove(bp_id);
        match pending {
            Some(p) => {
                // 发送失败 = 接收端已超时放行，提示前端即可
                p.tx.send(decision).map_err(|_| "断点已超时自动放行".to_string())
            }
            None => Err("断点不存在或已被处理".to_string()),
        }
    }

    /// 处理单个 TCP 连接。
    async fn handle_connection<S>(
        self: &Arc<Self>,
        stream: S,
        client_ip: String,
        client_app: Option<String>,
    ) -> Result<(), String>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        // 通道：service 闭包把 CONNECT 的 upgrade 上下文发给连接循环。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<UpgradeContext>();

        let engine = self.clone();
        let client_ip_inner = client_ip.clone();
        let client_app_inner = client_app.clone();
        let service = service_fn(move |req: Request<Incoming>| {
            let engine = engine.clone();
            let tx = tx.clone();
            let client_ip = client_ip_inner.clone();
            let client_app = client_app_inner.clone();
            async move { engine.handle_request(req, tx, client_ip, client_app).await }
        });

        let io = TokioIo::new(stream);
        let conn = http1::Builder::new()
            .preserve_header_case(true)
            .serve_connection(io, service)
            .with_upgrades();

        tokio::pin!(conn);

        // 主循环：驱动连接，并处理 CONNECT 升级。
        // 关键：conn 因 upgrade 返回 Ok 时，on_upgrade 已就绪，需继续消费 rx 里的上下文。
        let mut pending_upgrade: Option<(
            hyper::upgrade::OnUpgrade,
            String,
            Arc<CertificateAuthority>,
            Arc<ProxyEngine>,
            String,
            Option<String>,
        )> = None;

        loop {
            tokio::select! {
                res = &mut conn => {
                    match res {
                        Ok(()) => {
                            // conn 可能因 upgrade 或正常关闭而返回。
                            // 先消费 rx 中可能存在的 upgrade 上下文，再决定是否退出。
                            while let Ok(ctx) = rx.try_recv() {
                                pending_upgrade = Some((ctx.on_upgrade, ctx.host, ctx.ca, ctx.engine, ctx.client_ip, ctx.client_app));
                            }
                            // 若有待处理的 upgrade，继续处理；否则正常退出。
                            if pending_upgrade.is_none() {
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("[proxy] connection error: {e}");
                            break;
                        }
                    }
                }
                resolved = async {
                    match &mut pending_upgrade {
                        Some((on_upgrade, _, _, _, _, _)) => Some(on_upgrade.await),
                        None => std::future::pending::<Option<Result<hyper::upgrade::Upgraded, hyper::Error>>>().await,
                    }
                } => {
                    if let Some(r) = resolved {
                        if let Some((_, host, ca, engine, client_ip, client_app)) = pending_upgrade.take() {
                            match r {
                                Ok(upgraded) => {
                                    tauri::async_runtime::spawn(async move {
                                        let _ = engine.handle_tls_upgrade(upgraded, host, ca, client_ip, client_app).await;
                                    });
                                }
                                Err(e) => {
                                    eprintln!("[proxy] upgrade error: {e}");
                                }
                            }
                        }
                    }
                }
                ctx = rx.recv() => {
                    if let Some(ctx) = ctx {
                        pending_upgrade = Some((ctx.on_upgrade, ctx.host, ctx.ca, ctx.engine, ctx.client_ip, ctx.client_app));
                    }
                }
            }
        }

        Ok(())
    }

    /// 处理单个 HTTP 层请求。
    async fn handle_request(
        self: Arc<Self>,
        req: Request<Incoming>,
        tx: tokio::sync::mpsc::UnboundedSender<UpgradeContext>,
        client_ip: String,
        client_app: Option<String>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        // 门户：目标是代理自身的请求（手机扫码下载证书、引导页、自检）
        if req.method() != Method::CONNECT {
            let portal = self.portal.lock().unwrap().clone();
            if let Some(portal) = portal {
                let host_header = req
                    .headers()
                    .get("host")
                    .and_then(|v| v.to_str().ok());
                if portal.is_self_target(req.uri(), host_header) {
                    return Ok(portal::handle(&portal, req).await);
                }
            }
        }

        // HTTPS CONNECT：建立中间人隧道
        if req.method() == Method::CONNECT {
            return match self.handle_connect(req, tx, client_ip, client_app).await {
                Ok(resp) => Ok(resp),
                Err(e) => {
                    eprintln!("[proxy] CONNECT error: {e}");
                    Ok(Response::builder()
                        .status(500)
                        .body(Full::new(Bytes::from(e)))
                        .unwrap())
                }
            };
        }

        self.handle_plain_http(req, None, false, client_ip, client_app).await
    }

    /// 处理普通 HTTP（或已解密的 HTTPS）请求。
    ///
    /// - `host_hint`：TLS 解密后的 origin-form 请求没有 host，由 CONNECT 目标提供
    /// - `via_tls`：是否经过 HTTPS 中间人解密
    async fn handle_plain_http(
        self: Arc<Self>,
        req: Request<Incoming>,
        host_hint: Option<String>,
        via_tls: bool,
        client_ip: String,
        client_app: Option<String>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        let started = Instant::now();
        let started_at = now_ms();
        let id = Uuid::new_v4().to_string();

        let method = req.method().clone();
        let uri = req.uri().clone();

        // 解析 host / scheme / 展示 URL
        let (host, scheme, display_url) = match uri.host() {
            Some(h) => {
                let scheme = uri.scheme_str().unwrap_or("http").to_string();
                (h.to_string(), scheme, uri.to_string())
            }
            None => {
                let h = host_hint.unwrap_or_else(|| "unknown".to_string());
                let scheme = if via_tls { "https" } else { "http" }.to_string();
                let path_q = uri
                    .path_and_query()
                    .map(|p| p.as_str().to_string())
                    .unwrap_or_else(|| "/".to_string());
                let url = format!("{scheme}://{h}{path_q}");
                (h, scheme, url)
            }
        };

        // WebSocket 升级检测
        let is_websocket = req
            .headers()
            .get("upgrade")
            .map(|v| v.to_str().unwrap_or("").to_lowercase().contains("websocket"))
            .unwrap_or(false);

        if is_websocket {
            return self
                .handle_websocket(
                    req,
                    client_ip,
                    client_app,
                    host,
                    scheme,
                    via_tls,
                    id,
                    started,
                    started_at,
                    display_url,
                )
                .await;
        }

        // ===== 普通请求转发 =====

        // 读取请求体
        let (parts, body) = req.into_parts();
        let request_headers: Vec<(String, String)> = parts
            .headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let request_body_bytes = match body.collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => Bytes::new(),
        };

        // 提取请求的 content-encoding / content-type
        let req_content_encoding = request_headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == "content-encoding")
            .map(|(_, v)| v.clone());
        let req_content_type = request_headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == "content-type")
            .map(|(_, v)| v.clone());

        let request_body = if request_body_bytes.is_empty() {
            None
        } else if is_text_content(req_content_type.as_deref()) {
            let decoded = decode_body(&request_body_bytes, req_content_encoding.as_deref());
            Some(body_to_string(&decoded))
        } else {
            Some(format!(
                "[二进制内容，{} 字节，Content-Type: {}]",
                request_body_bytes.len(),
                req_content_type.clone().unwrap_or_else(|| "unknown".to_string())
            ))
        };

        // ===== 规则引擎 =====
        // 断点可能修改请求的任何部分，因此这些变量在此可变
        let (mut method, mut display_url, mut request_headers, mut request_body, mut request_body_bytes) =
            (method, display_url, request_headers, request_body, request_body_bytes);

        let path_for_match = uri.path().to_string();
        let matched = self.rules.match_rule(&host, &path_for_match, method.as_str());
        let mut matched_rule_name: Option<String> = None;
        let mut delay_response_ms: Option<u64> = None;
        let mut throttle: Option<(u32, u64, u8)> = None;

        if let Some(rule) = matched {
            matched_rule_name = Some(rule.name.clone());
            match rule.action {
                RuleAction::Mock { status, content_type, body } => {
                    let resp_headers = vec![
                        ("content-type".to_string(), content_type.clone()),
                        ("x-paxi-rule".to_string(), rule.name.clone()),
                    ];
                    let record = RequestRecord {
                        id,
                        client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                        method: method.to_string(),
                        url: display_url.clone(),
                        host: host.clone(),
                        scheme: scheme.clone(),
                        status,
                        request_headers: request_headers.clone(),
                        response_headers: resp_headers.clone(),
                        request_body: request_body.clone(),
                        response_body: Some(body.clone()),
                        request_body_size: request_body_bytes.len() as u64,
                        response_body_size: body.len() as u64,
                        content_type: Some(content_type),
                        duration_ms: started.elapsed().as_millis(),
                        started_at,
                        error: None,
                        is_websocket: false,
                        ws_frame_count: 0,
                        matched_rule: matched_rule_name.clone(),
                        is_replay: false,
                        passthrough: false,
                    };
                    self.record(record);
                    let mut b = Response::builder().status(status);
                    for (k, v) in &resp_headers {
                        b = b.header(k.as_str(), v.as_str());
                    }
                    return Ok(b.body(Full::new(Bytes::from(body))).unwrap());
                }
                RuleAction::Redirect { to, status } => {
                    let resp_headers = vec![
                        ("location".to_string(), to.clone()),
                        ("content-length".to_string(), "0".to_string()),
                        ("x-paxi-rule".to_string(), rule.name.clone()),
                    ];
                    let record = RequestRecord {
                        id,
                        client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                        method: method.to_string(),
                        url: display_url.clone(),
                        host: host.clone(),
                        scheme: scheme.clone(),
                        status,
                        request_headers: request_headers.clone(),
                        response_headers: resp_headers.clone(),
                        request_body: request_body.clone(),
                        response_body: None,
                        request_body_size: request_body_bytes.len() as u64,
                        response_body_size: 0,
                        content_type: None,
                        duration_ms: started.elapsed().as_millis(),
                        started_at,
                        error: None,
                        is_websocket: false,
                        ws_frame_count: 0,
                        matched_rule: matched_rule_name.clone(),
                        is_replay: false,
                        passthrough: false,
                    };
                    self.record(record);
                    let mut b = Response::builder().status(status);
                    for (k, v) in &resp_headers {
                        b = b.header(k.as_str(), v.as_str());
                    }
                    return Ok(b.body(Full::new(Bytes::new())).unwrap());
                }
                RuleAction::Abort => {
                    let body_text = format!("Blocked by paxi rule: {}", rule.name);
                    let resp_headers = vec![
                        ("content-type".to_string(), "text/plain; charset=utf-8".to_string()),
                        ("x-paxi-rule".to_string(), rule.name.clone()),
                    ];
                    let record = RequestRecord {
                        id,
                        client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                        method: method.to_string(),
                        url: display_url.clone(),
                        host: host.clone(),
                        scheme: scheme.clone(),
                        status: 403,
                        request_headers: request_headers.clone(),
                        response_headers: resp_headers.clone(),
                        request_body: request_body.clone(),
                        response_body: Some(body_text.clone()),
                        request_body_size: request_body_bytes.len() as u64,
                        response_body_size: body_text.len() as u64,
                        content_type: Some("text/plain".to_string()),
                        duration_ms: started.elapsed().as_millis(),
                        started_at,
                        error: None,
                        is_websocket: false,
                        ws_frame_count: 0,
                        matched_rule: matched_rule_name.clone(),
                        is_replay: false,
                        passthrough: false,
                    };
                    self.record(record);
                    let mut b = Response::builder().status(403);
                    for (k, v) in &resp_headers {
                        b = b.header(k.as_str(), v.as_str());
                    }
                    return Ok(b.body(Full::new(Bytes::from(body_text))).unwrap());
                }
                RuleAction::Delay { ms } => {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }
                RuleAction::DelayResponse { ms } => {
                    delay_response_ms = Some(ms);
                }
                RuleAction::Throttle { kbps, delay_ms, drop_pct } => {
                    throttle = Some((kbps, delay_ms, drop_pct));
                }
                RuleAction::Breakpoint => {
                    // 挂起等待前端决策：放行（可改请求）/ 拦截；超时按原样放行
                    let decision = self
                        .wait_breakpoint(
                            &id,
                            method.as_str(),
                            &display_url,
                            request_headers.clone(),
                            request_body.clone(),
                            started_at,
                        )
                        .await;
                    match decision {
                        Some(BreakpointDecision::Forward { method: m, url: u, headers: h, body: b }) => {
                            if let Ok(parsed) = m.parse() {
                                method = parsed;
                            }
                            display_url = u;
                            request_headers = h.into_iter().filter(|(k, v)| !k.is_empty() && !v.is_empty()).collect();
                            request_body_bytes = b.map(Bytes::from).unwrap_or_default();
                            request_body = if request_body_bytes.is_empty() {
                                None
                            } else {
                                Some(body_to_string(&request_body_bytes))
                            };
                        }
                        Some(BreakpointDecision::Abort) => {
                            let body_text = format!("Blocked at paxi breakpoint: {}", rule.name);
                            let resp_headers = vec![
                                ("content-type".to_string(), "text/plain; charset=utf-8".to_string()),
                                ("x-paxi-breakpoint".to_string(), rule.name.clone()),
                            ];
                            let record = RequestRecord {
                                id,
                                client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                                method: method.to_string(),
                                url: display_url.clone(),
                                host: host.clone(),
                                scheme: scheme.clone(),
                                status: 403,
                                request_headers,
                                response_headers: resp_headers.clone(),
                                request_body,
                                response_body: Some(body_text.clone()),
                                request_body_size: request_body_bytes.len() as u64,
                                response_body_size: body_text.len() as u64,
                                content_type: Some("text/plain".to_string()),
                                duration_ms: started.elapsed().as_millis(),
                                started_at,
                                error: None,
                                is_websocket: false,
                                ws_frame_count: 0,
                                matched_rule: matched_rule_name.clone(),
                                is_replay: false,
                                passthrough: false,
                            };
                            self.record(record);
                            let mut b = Response::builder().status(403);
                            for (k, v) in &resp_headers {
                                b = b.header(k.as_str(), v.as_str());
                            }
                            return Ok(b.body(Full::new(Bytes::from(body_text))).unwrap());
                        }
                        None => { /* 超时/通道关闭：按原样放行 */ }
                    }
                }
            }
        }

        // 构造转发请求：display_url 恒为绝对形式（断点放行后可能已被修改）
        // 弱网：转发前模拟首字节延迟
        if let Some((_, delay_ms, _)) = throttle {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
        let _uri_check: hyper::Uri = match display_url.parse() {
                Ok(u) => u,
                Err(e) => {
                    let record = RequestRecord {
                        id,
                        client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                        method: method.to_string(),
                        url: display_url.clone(),
                        host,
                        scheme,
                        status: 400,
                        request_headers,
                        response_headers: vec![],
                        request_body,
                        response_body: None,
                        request_body_size: request_body_bytes.len() as u64,
                        response_body_size: 0,
                        content_type: None,
                        duration_ms: started.elapsed().as_millis(),
                        started_at,
                        error: Some(format!("构造 URI 失败：{e}")),
                        is_websocket: false,
                        ws_frame_count: 0,
                        matched_rule: matched_rule_name.clone(),
                        is_replay: false,
                        passthrough: false,
                    };
                    self.record(record);
                    return Ok(Response::builder()
                        .status(400)
                        .body(Full::new(Bytes::from("Bad Request")))
                        .unwrap());
                }
        };

        // 转发（直连或上游代理）
        match self
            .send_request(
                method.as_str(),
                &display_url,
                &request_headers,
                request_body_bytes.clone(),
            )
            .await
        {
            Ok((status, response_headers, mut response_body_bytes)) => {
                // 弱网：按概率随机"丢包"（截断响应体，模拟传输中断）
                let mut truncated = false;
                if let Some((_, _, drop_pct)) = throttle {
                    if drop_pct > 0 {
                        let roll = (now_ms() % 100) as u8; // 简单伪随机：足够模拟
                        if roll < drop_pct {
                            let keep = response_body_bytes.len() / 2;
                            response_body_bytes = response_body_bytes.slice(0..keep);
                            truncated = true;
                        }
                    }
                }

                // 提取 content-encoding / content-type
                let content_encoding = response_headers
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == "content-encoding")
                    .map(|(_, v)| v.clone());
                let content_type = response_headers
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == "content-type")
                    .map(|(_, v)| v.split(';').next().unwrap_or("").trim().to_string());

                // 解压 + 判断是否文本，决定如何存储
                let response_body = if response_body_bytes.is_empty() {
                    None
                } else if is_text_content(content_type.as_deref()) {
                    let decoded = decode_body(&response_body_bytes, content_encoding.as_deref());
                    Some(body_to_string(&decoded))
                } else {
                    let size = response_body_bytes.len();
                    Some(format!(
                        "[二进制内容，{} 字节，Content-Type: {}]",
                        size,
                        content_type.clone().unwrap_or_else(|| "unknown".to_string())
                    ))
                };

                let record = RequestRecord {
                    id,
                    client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                    method: method.to_string(),
                    url: display_url.clone(),
                    host,
                    scheme,
                    request_body,
                    response_body,
                    status,
                    request_headers,
                    response_headers: response_headers.clone(),
                    request_body_size: request_body_bytes.len() as u64,
                    response_body_size: response_body_bytes.len() as u64,
                    content_type,
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: None,
                    is_websocket: false,
                    ws_frame_count: 0,
                    matched_rule: matched_rule_name.clone(),
                    is_replay: false,
                    passthrough: false,
                };
                self.record(record);

                // 响应延迟规则：返回客户端前等待
                if let Some(ms) = delay_response_ms {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }

                // 弱网：带宽限速——按响应体大小与带宽上限模拟传输耗时
                if let Some((kbps, _, _)) = throttle {
                    if kbps > 0 && !response_body_bytes.is_empty() {
                        let transfer_ms = (response_body_bytes.len() as u64) * 1000 / (kbps as u64 * 1024);
                        if transfer_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(transfer_ms)).await;
                        }
                    }
                }

                // 重建响应返回给客户端（截断时重写 content-length 保持一致）
                let mut builder = Response::builder().status(status);
                for (k, v) in &response_headers {
                    if truncated && k.eq_ignore_ascii_case("content-length") {
                        builder = builder.header(k.as_str(), response_body_bytes.len().to_string());
                    } else {
                        builder = builder.header(k.as_str(), v.as_str());
                    }
                }
                Ok(builder
                    .body(Full::new(response_body_bytes))
                    .unwrap_or_else(|_| {
                        Response::builder()
                            .status(500)
                            .body(Full::new(Bytes::from("Internal Error")))
                            .unwrap()
                    }))
            }
            Err(e) => {
                let msg = format!("转发失败：{e}");
                let record = RequestRecord {
                    id,
                    client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                    method: method.to_string(),
                    url: display_url.clone(),
                    host,
                    scheme,
                    status: 0,
                    request_headers,
                    response_headers: vec![],
                    request_body,
                    response_body: None,
                    request_body_size: request_body_bytes.len() as u64,
                    response_body_size: 0,
                    content_type: None,
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: Some(msg.clone()),
                    is_websocket: false,
                    ws_frame_count: 0,
                    matched_rule: matched_rule_name.clone(),
                    is_replay: false,
                    passthrough: false,
                };
                self.record(record);
                Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from(msg)))
                    .unwrap())
            }
        }
    }

    /// 处理 WebSocket 升级请求：
    /// 上游直连真实服务器，客户端侧手动完成 101 握手，随后双向转发并逐帧记录。
    #[allow(clippy::too_many_arguments)]
    async fn handle_websocket(
        self: Arc<Self>,
        mut req: Request<Incoming>,
        client_ip: String,
        client_app: Option<String>,
        host: String,
        scheme: String,
        via_tls: bool,
        id: String,
        started: Instant,
        started_at: u128,
        display_url: String,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        // 先取 upgrade future（必须在 into_parts 之前）
        let on_upgrade = hyper::upgrade::on(&mut req);

        let (parts, body) = req.into_parts();
        let request_headers: Vec<(String, String)> = parts
            .headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        // WebSocket 握手请求没有 body，消费掉以保持连接状态干净
        let _ = body.collect().await;

        let header = |name: &str| -> Option<String> {
            request_headers
                .iter()
                .find(|(k, _)| k.to_lowercase() == name)
                .map(|(_, v)| v.clone())
        };

        let client_key = header("sec-websocket-key");
        let Some(client_key) = client_key else {
            let record = RequestRecord {
                id,
                client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                method: "GET".to_string(),
                url: display_url,
                host,
                scheme,
                status: 400,
                request_headers,
                response_headers: vec![],
                request_body: None,
                response_body: None,
                request_body_size: 0,
                response_body_size: 0,
                content_type: None,
                duration_ms: started.elapsed().as_millis(),
                started_at,
                error: Some("缺少 Sec-WebSocket-Key".to_string()),
                is_websocket: true,
                ws_frame_count: 0,
                matched_rule: None,
                is_replay: false,
                passthrough: false,
            };
            self.record(record);
            return Ok(Response::builder()
                .status(400)
                .body(Full::new(Bytes::from("Missing Sec-WebSocket-Key")))
                .unwrap());
        };

        // 上游 WebSocket URL
        let path_q = parts
            .uri
            .path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| "/".to_string());
        // URL 里可能带端口（明文代理 ws://host:port/path），origin-form 时用 CONNECT 的 host
        let authority = match parts.uri.host() {
            Some(h) => {
                let port = parts.uri.port_u16();
                match port {
                    Some(p) => format!("{h}:{p}"),
                    None => h.to_string(),
                }
            }
            None => host.clone(),
        };
        let ws_url = if via_tls {
            format!("wss://{authority}{path_q}")
        } else {
            format!("ws://{authority}{path_q}")
        };

        match ws::connect_upstream(&ws_url, &request_headers).await {
            Ok((upstream_ws, protocol)) => {
                // 给客户端的 101 响应
                let response = ws::switch_response(&client_key, protocol.as_deref());
                let response_headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                // 记录（握手即入库，列表立即可见；帧数随后实时累计）
                let record = RequestRecord {
                    id: id.clone(),
                    client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                    method: "GET".to_string(),
                    url: ws_url.clone(),
                    host: authority.clone(),
                    scheme: if via_tls { "wss".to_string() } else { "ws".to_string() },
                    status: 101,
                    request_headers,
                    response_headers,
                    request_body: None,
                    response_body: None,
                    request_body_size: 0,
                    response_body_size: 0,
                    content_type: None,
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: None,
                    is_websocket: true,
                    ws_frame_count: 0,
                    matched_rule: None,
                    is_replay: false,
                    passthrough: false,
                };
                let meta = self.store.insert(record);
                self.hub.push_meta(meta.clone());

                // 升级客户端连接后开始双向转发
                let store = self.store.clone();
                let hub = self.hub.clone();
                tauri::async_runtime::spawn(async move {
                    let upgraded = match on_upgrade.await {
                        Ok(u) => u,
                        Err(e) => {
                            eprintln!("[ws] client upgrade failed: {e}");
                            return;
                        }
                    };
                    let client_ws = WebSocketStream::from_raw_socket(
                        TokioIo::new(upgraded),
                        tokio_tungstenite::tungstenite::protocol::Role::Server,
                        None,
                    )
                    .await;
                    ws::pump_and_record(
                        id,
                        started_at,
                        meta,
                        store,
                        hub,
                        client_ws,
                        upstream_ws,
                    )
                    .await;
                });

                Ok(response)
            }
            Err(e) => {
                let msg = format!("WebSocket 上游连接失败：{e}");
                let record = RequestRecord {
                    id,
                    client_ip: Some(client_ip),
                        client_process: client_app.clone(),
                    method: "GET".to_string(),
                    url: ws_url,
                    host: authority,
                    scheme: if via_tls { "wss".to_string() } else { "ws".to_string() },
                    status: 502,
                    request_headers,
                    response_headers: vec![],
                    request_body: None,
                    response_body: None,
                    request_body_size: 0,
                    response_body_size: 0,
                    content_type: None,
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: Some(msg.clone()),
                    is_websocket: true,
                    ws_frame_count: 0,
                    matched_rule: None,
                    is_replay: false,
                    passthrough: false,
                };
                self.record(record);
                Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from(msg)))
                    .unwrap())
            }
        }
    }

    /// 处理 HTTPS CONNECT：返回 200 并通过通道发送 upgrade 上下文。
    async fn handle_connect(
        self: Arc<Self>,
        mut req: Request<Incoming>,
        tx: tokio::sync::mpsc::UnboundedSender<UpgradeContext>,
        client_ip: String,
        client_app: Option<String>,
    ) -> Result<Response<Full<Bytes>>, String> {
        // CONNECT 的目标 host:port
        let host_port = req.uri().to_string();
        let (host, _port) = parse_host_port(&host_port);

        let on_upgrade = hyper::upgrade::on(&mut req);

        // 把 upgrade 上下文发送给连接循环
        let _ = tx.send(UpgradeContext {
            on_upgrade,
            host,
            ca: self.ca.clone(),
            engine: self.clone(),
            client_ip,
            client_app,
        });

        // 返回 200 Connection Established
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::new()))
            .unwrap())
    }

    /// 在升级流上做 TLS 握手，然后作为内层 HTTP server 处理解密后的请求。
    /// 命中直通列表时跳过 MITM，直接与上游建立 TCP 隧道双向转发。
    async fn handle_tls_upgrade(
        self: Arc<Self>,
        stream: hyper::upgrade::Upgraded,
        host: String,
        ca: Arc<CertificateAuthority>,
        client_ip: String,
        client_app: Option<String>,
    ) -> Result<(), String> {
        let started_at = now_ms();

        // ===== TLS 直通：不做 MITM，直接隧道转发 =====
        if self.is_passthrough(&host) {
            let engine = self.clone();
            let host_inner = host.clone();
            let app_inner = client_app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = engine
                    .passthrough_tunnel(stream, &host_inner, client_ip, app_inner, started_at)
                    .await;
            });
            return Ok(());
        }

        // 为 host 签发叶子证书，建立 TLS acceptor
        let acceptor = match build_tls_acceptor(&ca, &host) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("证书签发失败：{e}");
                self.record_tls_failure(&host, &client_ip, &client_app, started_at, &msg);
                return Err(msg);
            }
        };
        // Upgraded 实现的是 hyper::rt 的 trait，用 TokioIo 包装成 tokio 的 trait
        let tls_stream = match acceptor.accept(TokioIo::new(stream)).await {
            Ok(s) => s,
            Err(e) => {
                // 握手失败最常见原因：客户端不信任我们的根证书，或 App 做了证书校验
                let msg = format!(
                    "TLS 握手失败（{e}）：客户端可能未安装/未信任 paxi 根证书；若已信任仍失败，该 App 可能做了证书校验（SSL Pinning），可将 {host} 加入直通列表"
                );
                self.record_tls_failure(&host, &client_ip, &client_app, started_at, &msg);
                return Err(e.to_string());
            }
        };

        // 在 TLS 流上跑内层 HTTP server，处理解密后的请求
        let engine = self.clone();
        let host_inner = host.clone();
        let client_ip_inner = client_ip.clone();
        let client_app_inner = client_app.clone();
        let service = service_fn(move |req: Request<Incoming>| {
            let engine = engine.clone();
            let host = host_inner.clone();
            let client_ip = client_ip_inner.clone();
            let client_app = client_app_inner.clone();
            async move { engine.handle_decrypted_request(req, host, client_ip, client_app).await }
        });

        match http1::Builder::new()
            .preserve_header_case(true)
            .serve_connection(TokioIo::new(tls_stream), service)
            .with_upgrades()
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = format!("HTTPS 连接解析失败（{host}）：{e}");
                self.record_tls_failure(&host, &client_ip, &client_app, started_at, &msg);
                Err(e.to_string())
            }
        }
    }

    /// TLS 直通隧道：与上游建立 TCP 连接后双向透传字节。
    /// App 证书校验（SSL Pinning）可通过，但该域名流量内容不可见。
    async fn passthrough_tunnel(
        self: &Arc<Self>,
        client: hyper::upgrade::Upgraded,
        host: &str,
        client_ip: String,
        client_app: Option<String>,
        started_at: u128,
    ) -> Result<(), String> {
        let addr = format!("{host}:443");
        let upstream = match tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                let msg = format!("直通连接上游失败（{addr}）：{e}");
                self.record_passthrough(host, &client_ip, &client_app, started_at, Some(&msg));
                return Err(msg);
            }
            Err(_) => {
                let msg = format!("直通连接上游超时（{addr}）");
                self.record_passthrough(host, &client_ip, &client_app, started_at, Some(&msg));
                return Err(msg);
            }
        };

        // Upgraded 只实现 hyper::rt 的 IO，用 TokioIo 转成 tokio 的；
        // TcpStream 原生实现 tokio AsyncRead/Write，直接用
        let mut client_io = TokioIo::new(client);
        let mut upstream_io = upstream;
        let _ = tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io).await;

        self.record_passthrough(host, &client_ip, &client_app, started_at, None);
        Ok(())
    }

    /// 记录一条 TLS 失败（列表可见，便于诊断"抓不到"问题）。
    fn record_tls_failure(
        &self,
        host: &str,
        client_ip: &str,
        client_app: &Option<String>,
        started_at: u128,
        error: &str,
    ) {
        let record = RequestRecord {
            id: Uuid::new_v4().to_string(),
            client_ip: Some(client_ip.to_string()),
            client_process: client_app.clone(),
            method: "CONNECT".to_string(),
            url: format!("https://{host}/"),
            host: host.to_string(),
            scheme: "https".to_string(),
            status: 0,
            request_headers: vec![],
            response_headers: vec![],
            request_body: None,
            response_body: None,
            request_body_size: 0,
            response_body_size: 0,
            content_type: None,
            duration_ms: now_ms().saturating_sub(started_at),
            started_at,
            error: Some(error.to_string()),
            is_websocket: false,
            ws_frame_count: 0,
            matched_rule: None,
            is_replay: false,
            passthrough: false,
        };
        self.record(record);
    }

    /// 记录一条直通转发（隧道结束或失败时）。
    fn record_passthrough(
        &self,
        host: &str,
        client_ip: &str,
        client_app: &Option<String>,
        started_at: u128,
        error: Option<&str>,
    ) {
        let record = RequestRecord {
            id: Uuid::new_v4().to_string(),
            client_ip: Some(client_ip.to_string()),
            client_process: client_app.clone(),
            method: "CONNECT".to_string(),
            url: format!("https://{host}/"),
            host: host.to_string(),
            scheme: "https".to_string(),
            status: 200,
            request_headers: vec![],
            response_headers: vec![],
            request_body: None,
            response_body: None,
            request_body_size: 0,
            response_body_size: 0,
            content_type: None,
            duration_ms: now_ms().saturating_sub(started_at),
            started_at,
            error: error.map(|s| s.to_string()),
            is_websocket: false,
            ws_frame_count: 0,
            matched_rule: None,
            is_replay: false,
            passthrough: true,
        };
        self.record(record);
    }

    /// 处理解密后的内层 HTTPS 请求：origin-form，host 由 CONNECT 目标提供。
    async fn handle_decrypted_request(
        self: Arc<Self>,
        req: Request<Incoming>,
        host: String,
        client_ip: String,
        client_app: Option<String>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        self.handle_plain_http(req, Some(host), true, client_ip, client_app)
            .await
    }
}

/// 解析 `host:port`。
fn parse_host_port(host_port: &str) -> (String, u16) {
    if let Some((h, p)) = host_port.rsplit_once(':') {
        if let Ok(port) = p.parse::<u16>() {
            return (h.to_string(), port);
        }
    }
    (host_port.to_string(), 443)
}

impl ProxyEngine {
    fn record(&self, record: RequestRecord) {
        let meta = self.store.insert(record);
        self.hub.push_meta(meta);
    }

    /// 执行重放：直接用引擎 HTTP client 发送（不经代理监听端口），
    /// 结果作为新记录入库（is_replay = true）并推送前端。
    pub async fn execute_replay(
        self: &Arc<Self>,
        params: ReplayParams,
    ) -> Result<RequestMeta, String> {
        let started = Instant::now();
        let started_at = now_ms();
        let id = Uuid::new_v4().to_string();

        let uri: hyper::Uri = params
            .url
            .parse()
            .map_err(|e| format!("URL 无效：{e}"))?;
        let host = uri
            .host()
            .map(|h| h.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let scheme = uri.scheme_str().unwrap_or("https").to_string();
        let method = params.method.to_uppercase();

        // 过滤 hop-by-hop / 由 client 自管的头
        const SKIP: &[&str] = &[
            "connection",
            "proxy-connection",
            "keep-alive",
            "transfer-encoding",
            "upgrade",
            "content-length",
            "host",
        ];
        let headers: Vec<(String, String)> = params
            .headers
            .iter()
            .filter(|(k, _)| !SKIP.contains(&k.to_lowercase().as_str()))
            .cloned()
            .collect();

        let body_bytes = params.body.map(Bytes::from).unwrap_or_default();
        let req_content_type = headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == "content-type")
            .map(|(_, v)| v.clone());

        // 统一发送入口（直连或上游代理）
        match self
            .send_request(&method, &params.url, &headers, body_bytes.clone())
            .await
        {
            Ok((status, response_headers, response_body_bytes)) => {
                let content_encoding = response_headers
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == "content-encoding")
                    .map(|(_, v)| v.clone());
                let content_type = response_headers
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == "content-type")
                    .map(|(_, v)| v.split(';').next().unwrap_or("").trim().to_string());

                let response_body = if response_body_bytes.is_empty() {
                    None
                } else if is_text_content(content_type.as_deref()) {
                    let decoded = decode_body(&response_body_bytes, content_encoding.as_deref());
                    Some(body_to_string(&decoded))
                } else {
                    Some(format!(
                        "[二进制内容，{} 字节，Content-Type: {}]",
                        response_body_bytes.len(),
                        content_type.clone().unwrap_or_else(|| "unknown".to_string())
                    ))
                };

                let record = RequestRecord {
                    id,
                    client_ip: None,
                    client_process: None,
                    method,
                    url: params.url.clone(),
                    host,
                    scheme,
                    request_body: if body_bytes.is_empty() {
                        None
                    } else {
                        Some(body_to_string(&body_bytes))
                    },
                    response_body,
                    status,
                    request_headers: headers,
                    response_headers,
                    request_body_size: body_bytes.len() as u64,
                    response_body_size: response_body_bytes.len() as u64,
                    content_type,
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: None,
                    is_websocket: false,
                    ws_frame_count: 0,
                    matched_rule: None,
                    is_replay: true,
                    passthrough: false,
                };
                let meta = self.store.insert(record);
                self.hub.push_meta(meta.clone());
                Ok(meta)
            }
            Err(e) => {
                let msg = format!("重放失败：{e}");
                let record = RequestRecord {
                    id,
                    client_ip: None,
                    client_process: None,
                    method,
                    url: params.url.clone(),
                    host,
                    scheme,
                    request_body: if body_bytes.is_empty() {
                        None
                    } else {
                        Some(body_to_string(&body_bytes))
                    },
                    response_body: None,
                    status: 0,
                    request_headers: headers,
                    response_headers: vec![],
                    request_body_size: body_bytes.len() as u64,
                    response_body_size: 0,
                    content_type: req_content_type,
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: Some(msg),
                    is_websocket: false,
                    ws_frame_count: 0,
                    matched_rule: None,
                    is_replay: true,
                    passthrough: false,
                };
                let meta = self.store.insert(record);
                self.hub.push_meta(meta.clone());
                Ok(meta)
            }
        }
    }

    /// 统一的请求发送入口：优先走上游代理（手写 HTTP/1.1 隧道），否则 hyper 直连。
    /// 返回 (状态码, 响应头, 响应体字节)。
    async fn send_request(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Bytes,
    ) -> Result<(u16, Vec<(String, String)>, Bytes), String> {
        if self.upstream_enabled() {
            let upstream = self.upstream.lock().unwrap().clone().unwrap();
            self.forward_via_upstream(method, url, headers, body, &upstream)
                .await
        } else {
            self.send_direct(method, url, headers, body).await
        }
    }

    /// hyper 直连发送。
    async fn send_direct(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Bytes,
    ) -> Result<(u16, Vec<(String, String)>, Bytes), String> {
        let uri: hyper::Uri = url.parse().map_err(|e| format!("URL 无效：{e}"))?;
        let mut builder = Request::builder().method(method).uri(uri);
        for (k, v) in headers.iter().filter(|(k, _)| {
            !k.eq_ignore_ascii_case("proxy-connection")
                && !k.eq_ignore_ascii_case("connection")
                && !k.eq_ignore_ascii_case("content-length")
                && !k.eq_ignore_ascii_case("transfer-encoding")
        }) {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let req = builder
            .body(Full::new(body))
            .map_err(|e| format!("构造请求失败：{e}"))?;
        let resp = self
            .client
            .request(req)
            .await
            .map_err(|e| format!("转发失败：{e}"))?;
        let status = resp.status().as_u16();
        let (parts, resp_body) = resp.into_parts();
        let response_headers: Vec<(String, String)> = parts
            .headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let bytes = match resp_body.collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => Bytes::new(),
        };
        Ok((status, response_headers, bytes))
    }

    /// 手工经上游 HTTP 代理发送（绝对 URL + 可选 Proxy-Authorization），
    /// 解析响应（Content-Length 或 chunked）。
    async fn forward_via_upstream(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Bytes,
        upstream: &UpstreamProxy,
    ) -> Result<(u16, Vec<(String, String)>, Bytes), String> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let addr = format!("{}:{}", upstream.host, upstream.port);
        let mut stream = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| format!("连接上游代理超时（{addr}）"))?
        .map_err(|e| format!("连接上游代理失败（{addr}）：{e}"))?;

        // 构造请求行与头
        let mut header_text = format!("{method} {url} HTTP/1.1\r\n");
        // Host：从 URL authority 取，或保留原 host 头
        let mut has_host = false;
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("host") {
                has_host = true;
                header_text.push_str(&format!("{}: {}\r\n", normalize_header(k), v));
            }
        }
        if !has_host {
            if let Ok(uri) = url.parse::<hyper::Uri>() {
                if let Some(a) = uri.authority() {
                    header_text.push_str(&format!("Host: {}\r\n", a));
                }
            }
        }
        for (k, v) in headers.iter().filter(|(k, _)| {
            !k.eq_ignore_ascii_case("host")
                && !k.eq_ignore_ascii_case("proxy-connection")
                && !k.eq_ignore_ascii_case("proxy-authorization")
                && !k.eq_ignore_ascii_case("content-length")
                && !k.eq_ignore_ascii_case("transfer-encoding")
        }) {
            header_text.push_str(&format!("{}: {}\r\n", normalize_header(k), v));
        }
        // 上游代理认证
        if !upstream.username.is_empty() {
            let cred = format!("{}:{}", upstream.username, upstream.password);
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, cred.as_bytes());
            header_text.push_str(&format!("Proxy-Authorization: Basic {b64}\r\n"));
        }
        // body（content-length 由字节长度决定）
        header_text.push_str(&format!("Content-Length: {}\r\n", body.len()));
        header_text.push_str("Connection: close\r\n\r\n");

        stream
            .write_all(header_text.as_bytes())
            .await
            .map_err(|e| format!("写入上游代理失败：{e}"))?;
        if !body.is_empty() {
            stream
                .write_all(&body)
                .await
                .map_err(|e| format!("写入请求体失败：{e}"))?;
        }

        // 读响应头
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .await
                .map_err(|e| format!("读取响应头失败：{e}"))?;
            if n == 0 {
                return Err("上游代理意外关闭连接（响应头不完整）".to_string());
            }
            head.push_str(&line);
            if line == "\r\n" || line == "\n" {
                break;
            }
            if head.len() > 64 * 1024 {
                return Err("响应头过大".to_string());
            }
        }

        // 解析状态行
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or("无法解析上游代理响应状态码")?;

        // 解析头
        let mut response_headers: Vec<(String, String)> = Vec::new();
        let mut content_length: Option<usize> = None;
        let mut chunked = false;
        for l in head.lines().skip(1) {
            if let Some((k, v)) = l.split_once(':') {
                let k = k.trim().to_string();
                let v = v.trim().to_string();
                if k.eq_ignore_ascii_case("content-length") {
                    content_length = v.parse().ok();
                }
                if k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked") {
                    chunked = true;
                }
                response_headers.push((k, v));
            }
        }

        // 读 body
        let body_bytes: Bytes = if chunked {
            let mut out = Vec::new();
            loop {
                let mut size_line = String::new();
                let n = reader
                    .read_line(&mut size_line)
                    .await
                    .map_err(|e| format!("读 chunk 失败：{e}"))?;
                if n == 0 {
                    break;
                }
                let size_str = size_line.trim().split(';').next().unwrap_or("0").trim();
                let size = usize::from_str_radix(size_str, 16).unwrap_or(0);
                if size == 0 {
                    // 读掉末尾 CRLF（以及可能的 trailer）
                    let _ = reader.read_line(&mut String::new()).await;
                    break;
                }
                let mut chunk = vec![0u8; size];
                reader
                    .read_exact(&mut chunk)
                    .await
                    .map_err(|e| format!("读 chunk 数据失败：{e}"))?;
                out.extend_from_slice(&chunk);
                let mut crlf = [0u8; 2];
                let _ = reader.read_exact(&mut crlf).await;
            }
            Bytes::from(out)
        } else if let Some(len) = content_length {
            let mut buf = vec![0u8; len];
            reader
                .read_exact(&mut buf)
                .await
                .map_err(|e| format!("读响应体失败：{e}"))?;
            Bytes::from(buf)
        } else {
            // 无长度信息：读到 EOF
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .await
                .map_err(|e| format!("读响应体失败：{e}"))?;
            Bytes::from(buf)
        };

        Ok((status, response_headers, body_bytes))
    }
}

/// 头名规范化（保留大小写首字母大写风格，HTTP 头不区分大小写，直接原样即可）。
fn normalize_header(k: &str) -> String {
    k.to_string()
}

/// 重放请求参数（来自前端编辑器）。
#[derive(serde::Deserialize)]
pub struct ReplayParams {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

// 辅助：为 TLS 配置构建动态证书的 acceptor。
fn build_tls_acceptor(ca: &CertificateAuthority, host: &str) -> Result<TlsAcceptor, String> {
    let (cert_pem, key_pem) = ca.leaf_for_host(host)?;
    let certs = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|e| e.to_string())?
        .ok_or("no private key")?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| e.to_string())?;

    // 声明 ALPN：明确告知客户端仅支持 HTTP/1.1。
    // 微信小程序 / NSURLSession / OkHttp 等会在 ClientHello 携带 ALPN（h2, http/1.1），
    // 服务端不响应 ALPN 时部分客户端会直接断连。声明后客户端会正常降级到 http/1.1。
    config.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(TlsAcceptor::from(Arc::new(config)))
}

// ServerName 的暂时占位引用，避免 unused import 警告。
#[allow(dead_code)]
fn _server_name(host: &str) -> ServerName<'static> {
    ServerName::try_from(host.to_string()).unwrap_or(ServerName::try_from("localhost").unwrap())
}
