//! HTTP/HTTPS 中间人代理引擎。
//!
//! - 监听 0.0.0.0:port（默认 8888）
//! - 普通 HTTP 请求：转发到真实服务器，记录请求/响应
//! - HTTPS CONNECT：与客户端用动态域名证书建 TLS，解密后按 HTTP 转发
//!
//! CONNECT 中间人流程：
//! 1. 收到 `CONNECT host:443`，返回 200 并拿到升级流
//! 2. 用 CA 为 host 签发叶子证书，在升级流上做 TLS 服务端握手
//! 3. 握手成功后，客户端发送真实的 HTTP（HTTPS）请求，此时走普通 HTTP 转发逻辑

use crate::ca::CertificateAuthority;
use crate::models::{body_to_string, RequestRecord, TrafficStore};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde::Serialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::rustls::{pki_types::ServerName, ServerConfig};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

/// 全局代理状态。
#[derive(Clone, Serialize)]
pub struct ProxyState {
    pub running: bool,
    pub port: u16,
    pub local_ip: String,
}

/// 代理引擎。
pub struct ProxyEngine {
    ca: Arc<CertificateAuthority>,
    store: Arc<TrafficStore>,
    /// 用于向真实服务器发请求的 HTTP 客户端（支持 HTTPS）。
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
    /// 当前 TCP 监听器（用于停止）。
    listener: Mutex<Option<Arc<TcpListener>>>,
    state: Mutex<ProxyState>,
}

impl ProxyEngine {
    pub fn new(ca: Arc<CertificateAuthority>, store: Arc<TrafficStore>) -> Arc<Self> {
        Arc::new(Self {
            ca,
            store,
            client: Client::builder(TokioExecutor::new()).build_http(),
            listener: Mutex::new(None),
            state: Mutex::new(ProxyState {
                running: false,
                port: 8888,
                local_ip: String::new(),
            }),
        })
    }

    /// 启动代理，监听 0.0.0.0:port。
    pub async fn start(self: &Arc<Self>, port: u16) -> Result<ProxyState, String> {
        self.stop().await;

        let addr: SocketAddr = format!("0.0.0.0:{port}")
            .parse::<SocketAddr>()
            .map_err(|e: std::net::AddrParseError| e.to_string())?;
        let listener = Arc::new(
            TcpListener::bind(&addr)
                .await
                .map_err(|e| format!("绑定端口 {port} 失败：{e}"))?,
        );

        let local_ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

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
                    Ok((stream, _peer)) => {
                        let engine = engine.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = engine.handle_connection(stream).await;
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

    /// 处理单个 TCP 连接。
    async fn handle_connection<S>(self: &Arc<Self>, stream: S) -> Result<(), String>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let service = service_fn(move |req: Request<Incoming>| {
            let engine = self.clone();
            async move { engine.handle_request(req).await }
        });

        let conn = http1::Builder::new()
            .preserve_header_case(true)
            .serve_connection(TokioIo::new(stream), service)
            .with_upgrades();

        // 处理升级（CONNECT 的中间人）
        tokio::pin!(conn);
        loop {
            match conn.as_mut().await {
                Ok(_) => break,
                Err(e) => {
                    eprintln!("[proxy] connection error: {e}");
                    break;
                }
            }
        }

        Ok(())
    }

    /// 处理单个 HTTP 层请求。
    async fn handle_request(
        self: Arc<Self>,
        req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        // HTTPS CONNECT：建立中间人隧道
        if req.method() == Method::CONNECT {
            return match self.handle_connect(req).await {
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

        self.handle_plain_http(req).await
    }

    /// 处理普通 HTTP（或已解密的 HTTPS）请求。
    async fn handle_plain_http(
        self: Arc<Self>,
        req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        let started = Instant::now();
        let started_at = chrono::Utc::now().timestamp_millis() as u128;
        let id = Uuid::new_v4().to_string();

        let method = req.method().clone();
        let uri = req.uri().clone();
        let host = uri
            .host()
            .map(|h| h.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let scheme = uri.scheme_str().unwrap_or("http").to_string();

        let is_websocket = req
            .headers()
            .get("upgrade")
            .map(|v| v.to_str().unwrap_or("").to_lowercase().contains("websocket"))
            .unwrap_or(false);

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
        let request_body = if request_body_bytes.is_empty() {
            None
        } else {
            Some(body_to_string(&request_body_bytes))
        };

        // 构造转发请求
        let mut forward_req = Request::builder()
            .method(parts.method.clone())
            .uri(parts.uri.clone());
        for (k, v) in &request_headers {
            if !k.to_lowercase().starts_with("proxy-connection") {
                forward_req = forward_req.header(k.as_str(), v.as_str());
            }
        }

        let forward_req = match forward_req.body(Full::new(request_body_bytes.clone())) {
            Ok(r) => r,
            Err(e) => {
                self.record(RequestRecord {
                    id,
                    method: method.to_string(),
                    url: uri.to_string(),
                    host,
                    scheme,
                    request_body,
                    response_body: None,
                    status: 0,
                    request_headers,
                    response_headers: vec![],
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: Some(format!("构造请求失败：{e}")),
                    is_websocket,
                });
                return Ok(Response::builder()
                    .status(400)
                    .body(Full::new(Bytes::from("Bad Request")))
                    .unwrap());
            }
        };

        // 转发
        match self.client.request(forward_req).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let status_code = resp.status();
                let (resp_parts, resp_body) = resp.into_parts();
                let response_headers: Vec<(String, String)> = resp_parts
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                let response_body_bytes = match resp_body.collect().await {
                    Ok(b) => b.to_bytes(),
                    Err(_) => Bytes::new(),
                };
                let response_body = if response_body_bytes.is_empty() {
                    None
                } else {
                    Some(body_to_string(&response_body_bytes))
                };

                self.record(RequestRecord {
                    id,
                    method: method.to_string(),
                    url: uri.to_string(),
                    host,
                    scheme,
                    request_body,
                    response_body: response_body.clone(),
                    status,
                    request_headers,
                    response_headers: response_headers.clone(),
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: None,
                    is_websocket,
                });

                // 重建响应返回给客户端
                let mut builder = Response::builder().status(status_code);
                for (k, v) in &response_headers {
                    builder = builder.header(k.as_str(), v.as_str());
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
                self.record(RequestRecord {
                    id,
                    method: method.to_string(),
                    url: uri.to_string(),
                    host,
                    scheme,
                    request_body,
                    response_body: None,
                    status: 0,
                    request_headers,
                    response_headers: vec![],
                    duration_ms: started.elapsed().as_millis(),
                    started_at,
                    error: Some(msg.clone()),
                    is_websocket,
                });
                Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from(msg)))
                    .unwrap())
            }
        }
    }

    /// 处理 HTTPS CONNECT：返回 200 并安排 TLS 中间人升级。
    async fn handle_connect(
        self: Arc<Self>,
        mut req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, String> {
        // CONNECT 的目标 host:port
        let host_port = req.uri().to_string();
        let (host, _port) = parse_host_port(&host_port);

        let on_upgrade = hyper::upgrade::on(&mut req);

        let engine = self.clone();
        let ca = self.ca.clone();
        // 后台处理升级：在升级流上做 TLS，然后跑内层 HTTP server
        tauri::async_runtime::spawn(async move {
            match on_upgrade.await {
                Ok(upgraded) => {
                    let _ = engine.handle_tls_upgrade(upgraded, host, ca).await;
                }
                Err(e) => {
                    eprintln!("[proxy] upgrade error: {e}");
                }
            }
        });

        // 返回 200 Connection Established
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::new()))
            .unwrap())
    }

    /// 在升级流上做 TLS 握手，然后作为内层 HTTP server 处理解密后的请求。
    async fn handle_tls_upgrade(
        self: Arc<Self>,
        stream: hyper::upgrade::Upgraded,
        host: String,
        ca: Arc<CertificateAuthority>,
    ) -> Result<(), String> {
        // 为 host 签发叶子证书，建立 TLS acceptor
        let acceptor = build_tls_acceptor(&ca, &host)?;
        // Upgraded 实现的是 hyper::rt 的 trait，用 TokioIo 包装成 tokio 的 trait
        let tls_stream = acceptor
            .accept(TokioIo::new(stream))
            .await
            .map_err(|e| e.to_string())?;

        // 在 TLS 流上跑内层 HTTP server，处理解密后的请求
        let engine = self.clone();
        let host_inner = host.clone();
        let service = service_fn(move |req: Request<Incoming>| {
            let engine = engine.clone();
            let host = host_inner.clone();
            async move { engine.handle_decrypted_request(req, &host).await }
        });

        http1::Builder::new()
            .preserve_header_case(true)
            .serve_connection(TokioIo::new(tls_stream), service)
            .await
            .map_err(|e| e.to_string())
    }

    /// 处理解密后的内层 HTTPS 请求：URI 是 origin-form，需要重建完整 URL 后转发。
    async fn handle_decrypted_request(
        self: Arc<Self>,
        mut req: Request<Incoming>,
        host: &str,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        let uri = req.uri().clone();
        // 重建完整 URL：https://host + origin-form
        let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let absolute = format!("https://{host}{path_and_query}");
        let absolute_uri: hyper::Uri = match absolute.parse() {
            Ok(u) => u,
            Err(_) => return Ok(Response::builder().status(400).body(Full::new(Bytes::from("Bad URI"))).unwrap()),
        };
        *req.uri_mut() = absolute_uri;

        self.handle_plain_http(req).await
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
        self.store.insert(record);
    }
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

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| e.to_string())?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

// ServerName 的暂时占位引用，避免 unused import 警告。
#[allow(dead_code)]
fn _server_name(host: &str) -> ServerName<'static> {
    ServerName::try_from(host.to_string()).unwrap_or(ServerName::try_from("localhost").unwrap())
}
