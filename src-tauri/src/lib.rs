pub mod ca;
pub mod models;
pub mod proxy;
pub mod system_proxy;

use ca::CertificateAuthority;
use models::{RequestMeta, RequestRecord, TrafficStore};
use proxy::{ProxyEngine, ProxyState};
use std::sync::Arc;
use tauri::Manager;

/// 应用全局状态：证书 + 流量存储 + 代理引擎。
struct AppState {
    ca: Arc<CertificateAuthority>,
    store: Arc<TrafficStore>,
    proxy: Arc<ProxyEngine>,
}

/// 初始化应用状态。
fn init_state(app: &tauri::AppHandle) -> Arc<AppState> {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());

    let ca = CertificateAuthority::load_or_create(&data_dir)
        .expect("failed to init CA certificate");
    let store = TrafficStore::new(1000);
    let proxy = ProxyEngine::new(ca.clone(), store.clone());

    Arc::new(AppState { ca, store, proxy })
}

// ===== Tauri Commands =====

/// 启动代理，并设置系统代理指向本机端口。
#[tauri::command]
async fn start_proxy(state: tauri::State<'_, Arc<AppState>>, port: u16) -> Result<ProxyState, String> {
    let result = state.proxy.start(port).await?;
    // 设置系统代理（Windows）
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

/// 获取请求列表（最新在前）。
#[tauri::command]
fn get_requests(state: tauri::State<'_, Arc<AppState>>) -> Vec<RequestMeta> {
    state.store.list()
}

/// 获取单条请求详情。
#[tauri::command]
fn get_request_detail(state: tauri::State<'_, Arc<AppState>>, id: String) -> Option<RequestRecord> {
    state.store.get(&id)
}

/// 清空请求记录。
#[tauri::command]
fn clear_requests(state: tauri::State<'_, Arc<AppState>>) {
    state.store.clear();
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
            clear_requests,
            export_ca_cert,
            get_ca_cert_pem,
            ai_chat
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
