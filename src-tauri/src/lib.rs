pub mod ca;
pub mod clients;
pub mod events;
pub mod har;
pub mod models;
pub mod portal;
pub mod process;
pub mod proxy;
pub mod rules;
pub mod storage;
pub mod stats;
pub mod system_proxy;
pub mod ws;

use ca::CertificateAuthority;
use clients::ClientInfo;
use events::EventHub;
use models::{RequestMeta, RequestRecord, WsFrame};
use proxy::{ProxyEngine, ProxyState, ReplayParams};
use rules::{Rule, RulesEngine};
use storage::TrafficStore;
use std::sync::Arc;
use tauri::Manager;

/// 应用全局状态：证书 + 存储 + 代理引擎 + 事件中心 + 客户端感知 + 规则。
struct AppState {
    ca: Arc<CertificateAuthority>,
    store: Arc<dyn TrafficStore>,
    proxy: Arc<ProxyEngine>,
    clients: Arc<clients::ClientTracker>,
    rules: Arc<RulesEngine>,
}

/// 初始化应用状态。
fn init_state(app: &tauri::AppHandle) -> Arc<AppState> {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());

    let ca = CertificateAuthority::load_or_create(&data_dir)
        .expect("failed to init CA certificate");

    // SQLite 落盘存储 + 大 body 文件目录
    let db_path = data_dir.join("traffic.db");
    let bodies_dir = data_dir.join("bodies");
    let store = storage::sqlite::SqliteStore::open(&db_path, &bodies_dir)
        .expect("failed to init traffic store");

    // 规则引擎（同一 DB 文件，独立连接）
    let rules = RulesEngine::open(&db_path).expect("failed to init rules engine");

    let hub = EventHub::new(app.clone());
    let clients = Arc::new(clients::ClientTracker::new(app.clone()));
    let proxy = ProxyEngine::new(ca.clone(), store.clone(), hub, clients.clone(), rules.clone());

    // 启动时恢复上游代理配置
    let upstream_path = data_dir.join("upstream.json");
    if let Ok(text) = std::fs::read_to_string(&upstream_path) {
        if let Ok(cfg) = serde_json::from_str::<crate::proxy::UpstreamProxy>(&text) {
            proxy.set_upstream(Some(cfg));
        }
    }

    Arc::new(AppState {
        ca,
        store,
        proxy,
        clients,
        rules,
    })
}

// ===== Tauri Commands =====

/// 启动代理，并设置系统代理指向本机端口。
#[tauri::command]
async fn start_proxy(state: tauri::State<'_, Arc<AppState>>, port: u16) -> Result<ProxyState, String> {
    let result = state.proxy.start(port).await?;
    // 设置系统代理（Windows / macOS）
    if let Err(e) = system_proxy::set_system_proxy(port) {
        eprintln!("[proxy] 设置系统代理失败（代理仍已启动）: {e}");
    }
    Ok(result)
}

/// 停止代理，并恢复系统代理。
#[tauri::command]
async fn stop_proxy(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    state.proxy.stop().await;
    // 恢复系统代理
    if let Err(e) = system_proxy::restore_system_proxy() {
        eprintln!("[proxy] 恢复系统代理失败: {e}");
    }
    Ok(())
}

/// 获取代理状态。
#[tauri::command]
fn get_proxy_status(state: tauri::State<'_, Arc<AppState>>) -> ProxyState {
    state.proxy.status()
}

/// 获取请求列表（最新在前；前端实时增量来自 traffic://new 事件）。
#[tauri::command]
fn get_requests(state: tauri::State<'_, Arc<AppState>>) -> Vec<RequestMeta> {
    state.store.list()
}

/// 获取单条请求详情。
#[tauri::command]
fn get_request_detail(state: tauri::State<'_, Arc<AppState>>, id: String) -> Option<RequestRecord> {
    state.store.get(&id)
}

/// 获取某条记录的全部 WebSocket 帧。
#[tauri::command]
fn get_ws_frames(state: tauri::State<'_, Arc<AppState>>, id: String) -> Vec<WsFrame> {
    state.store.frames(&id)
}

/// 获取已连接的客户端设备列表。
#[tauri::command]
fn get_clients(state: tauri::State<'_, Arc<AppState>>) -> Vec<ClientInfo> {
    state.clients.list()
}

/// ===== 规则 =====

/// 全部规则（按优先级降序）。
#[tauri::command]
fn list_rules(state: tauri::State<'_, Arc<AppState>>) -> Vec<Rule> {
    state.rules.list()
}

/// 新增或更新规则（立即生效）。
#[tauri::command]
fn upsert_rule(state: tauri::State<'_, Arc<AppState>>, rule: Rule) -> Result<(), String> {
    state.rules.upsert(rule)
}

/// 删除规则。
#[tauri::command]
fn delete_rule(state: tauri::State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.rules.delete(&id)
}

/// 重放请求：直接经引擎 client 发送，结果作为新记录（is_replay）入库。
#[tauri::command]
async fn replay_request(
    state: tauri::State<'_, Arc<AppState>>,
    params: ReplayParams,
) -> Result<RequestMeta, String> {
    state.proxy.execute_replay(params).await
}

/// 导出全部记录为 HAR（桌面 paxi-export-{timestamp}.har），返回路径与条数。
#[tauri::command]
fn export_har(app: tauri::AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    let desktop = app.path().desktop_dir().map_err(|e| e.to_string())?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = desktop.join(format!("paxi-export-{ts}.har"));
    let count = har::export_har(state.store.as_ref(), &path)?;
    Ok(format!("{}\n已导出 {count} 条记录", path.to_string_lossy()))
}

/// 获取 TLS 直通域名列表。
#[tauri::command]
fn get_passthrough_hosts(state: tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    state.proxy.passthrough_hosts()
}

/// 设置 TLS 直通域名列表。
#[tauri::command]
fn set_passthrough_hosts(state: tauri::State<'_, Arc<AppState>>, hosts: Vec<String>) {
    state.proxy.set_passthrough_hosts(hosts);
}

/// 当前挂起的断点列表。
#[tauri::command]
fn list_breakpoints(state: tauri::State<'_, Arc<AppState>>) -> Vec<crate::proxy::BreakpointInfo> {
    state.proxy.list_breakpoints()
}

/// 获取上游代理配置（无则返回空结构）。
#[tauri::command]
fn get_upstream_proxy(state: tauri::State<'_, Arc<AppState>>) -> crate::proxy::UpstreamProxy {
    state.proxy.upstream().unwrap_or_default()
}

/// 保存上游代理配置（持久化到数据目录 + 应用到引擎）。
#[tauri::command]
fn set_upstream_proxy(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    config: crate::proxy::UpstreamProxy,
) -> Result<(), String> {
    state.proxy.set_upstream(Some(config.clone()));
    // 持久化到 appdata/upstream.json
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    let path = dir.join("upstream.json");
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("保存上游代理配置失败：{e}"))?;
    Ok(())
}

/// 从 HAR 1.2 文件导入记录到流量库，返回导入条数。
#[tauri::command]
fn import_har(state: tauri::State<'_, Arc<AppState>>, path: String) -> Result<usize, String> {
    crate::har::import_har(state.store.as_ref(), std::path::Path::new(&path))
}

/// 恢复断点：应用放行（可修改请求）或拦截决策。
#[tauri::command]
fn resume_breakpoint(
    state: tauri::State<'_, Arc<AppState>>,
    bp_id: String,
    decision: crate::proxy::BreakpointDecision,
) -> Result<(), String> {
    state.proxy.resume_breakpoint(&bp_id, decision)
}

/// 清空请求记录。
#[tauri::command]
fn clear_requests(state: tauri::State<'_, Arc<AppState>>) {
    state.store.clear();
}

/// 流量统计（统计面板数据源）。
#[tauri::command]
fn get_stats(state: tauri::State<'_, Arc<AppState>>) -> crate::stats::Stats {
    crate::stats::compute(state.store.as_ref())
}

/// 导出根证书到桌面，返回完整路径。
#[tauri::command]
fn export_ca_cert(app: tauri::AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    let desktop = app
        .path()
        .desktop_dir()
        .map_err(|e| e.to_string())?;
    let path = desktop.join("paxi-root-ca.crt");
    state.ca.export_root_cert(&path)?;
    Ok(path.to_string_lossy().to_string())
}

/// 获取根证书 PEM 内容（供前端展示/复制）。
#[tauri::command]
fn get_ca_cert_pem(state: tauri::State<'_, Arc<AppState>>) -> String {
    state.ca.root_cert_pem()
}

/// AI 请求参数。
#[derive(serde::Deserialize)]
struct AiChatParams {
    base_url: String,
    api_key: String,
    model: String,
    messages: Vec<AiMessage>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct AiMessage {
    role: String,
    content: String,
}

/// 转发 AI 聊天请求（绕过浏览器 CORS 限制）。
#[tauri::command]
fn ai_chat(params: AiChatParams) -> Result<String, String> {
    let url = format!("{}/chat/completions", params.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": params.model,
        "messages": params.messages,
        "temperature": 0.3,
        "stream": false,
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&params.api_key)
        .json(&body)
        .send()
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;

    if !status.is_success() {
        return Err(format!("API 错误 ({status}): {}", &text[..text.len().min(300)]));
    }

    // 解析 choices[0].message.content
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("API 返回格式异常，未找到分析内容")?;
    Ok(content.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 显式安装 rustls 的 CryptoProvider（ring），避免多 provider 时自动检测失败
            let _ = rustls::crypto::ring::default_provider().install_default();
            let state = init_state(&app.handle());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_proxy,
            stop_proxy,
            get_proxy_status,
            get_requests,
            get_request_detail,
            get_stats,
            get_ws_frames,
            get_clients,
            clear_requests,
            export_ca_cert,
            get_ca_cert_pem,
            list_rules,
            upsert_rule,
            delete_rule,
            replay_request,
            export_har,
            get_passthrough_hosts,
            set_passthrough_hosts,
            list_breakpoints,
            resume_breakpoint,
            import_har,
            get_upstream_proxy,
            set_upstream_proxy,
            ai_chat
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
