//! 客户端设备感知：跟踪连接到代理的客户端 IP。
//!
//! 每当有新的客户端 IP 首次连入（或重新活跃），向前端推送 `clients://update`，
//! 前端工具栏可显示"已连接 N 台设备"。

use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// 客户端信息。
#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub ip: String,
    /// 是否本机（127.0.0.1 / ::1 / 本机局域网 IP）
    pub is_local: bool,
    /// 首次连入时间（epoch ms）
    pub first_seen: u128,
    /// 最近连入时间（epoch ms）
    pub last_seen: u128,
    /// 累计 TCP 连接数
    pub connections: u64,
}

/// 客户端跟踪器。
pub struct ClientTracker {
    /// None = 无头模式（测试）：不 emit
    app: Option<tauri::AppHandle>,
    local_ips: Vec<String>,
    clients: Mutex<HashMap<String, ClientInfo>>,
}

impl ClientTracker {
    pub fn new(app: AppHandle) -> Self {
        Self::with_app(Some(app))
    }

    /// 无头模式（集成测试用）。
    pub fn headless() -> Self {
        Self::with_app(None)
    }

    fn with_app(app: Option<AppHandle>) -> Self {
        // 收集本机所有 IP，用于标记 is_local
        let mut local_ips = vec!["127.0.0.1".to_string(), "::1".to_string(), "localhost".to_string()];
        if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
            for (_, ip) in ifaces {
                local_ips.push(ip.to_string());
            }
        }
        Self {
            app,
            local_ips,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// 记录一次来自某 IP 的 TCP 连接；若是新客户端则推送事件。
    pub fn track(&self, ip: IpAddr) {
        let ip_str = ip.to_string();
        let is_local = self.local_ips.iter().any(|l| l == &ip_str);
        let now = crate::models::now_ms();

        let mut clients = self.clients.lock().unwrap();
        let entry = clients.entry(ip_str.clone()).or_insert_with(|| ClientInfo {
            ip: ip_str.clone(),
            is_local,
            first_seen: now,
            last_seen: now,
            connections: 0,
        });
        entry.last_seen = now;
        entry.connections += 1;
        let snapshot = self.snapshot_locked(&clients);
        drop(clients);

        if let Some(app) = &self.app {
            let _ = app.emit("clients://update", &snapshot);
        }
    }

    /// 当前客户端列表（非本机在前，按最近活跃排序）。
    pub fn list(&self) -> Vec<ClientInfo> {
        let clients = self.clients.lock().unwrap();
        self.snapshot_locked(&clients)
    }

    fn snapshot_locked(&self, clients: &HashMap<String, ClientInfo>) -> Vec<ClientInfo> {
        let mut list: Vec<ClientInfo> = clients.values().cloned().collect();
        list.sort_by(|a, b| {
            b.is_local
                .cmp(&a.is_local)
                .then(b.last_seen.cmp(&a.last_seen))
        });
        list
    }
}
