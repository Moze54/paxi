//! paxi 命令行抓包模式（无 GUI）。
//!
//! 用法：
//!   paxi-cli [-p PORT] [-d DATA_DIR] [-i INTERVAL_MS]
//!
//! 行为：启动 headless 代理（与 GUI 同一引擎），周期打印新抓到的请求摘要，
//! Ctrl+C 停止并恢复系统代理。

use paxi_lib::ca::CertificateAuthority;
use paxi_lib::clients::ClientTracker;
use paxi_lib::events::EventHub;
use paxi_lib::proxy::ProxyEngine;
use paxi_lib::rules::RulesEngine;
use paxi_lib::storage::sqlite::SqliteStore;
use paxi_lib::storage::TrafficStore;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut port: u16 = 8888;
    let mut data_dir = PathBuf::from(".paxi-cli");
    let mut interval_ms: u64 = 1500;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--port" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    port = v.parse().unwrap_or(8888);
                }
            }
            "-d" | "--data-dir" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    data_dir = PathBuf::from(v);
                }
            }
            "-i" | "--interval" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    interval_ms = v.parse().unwrap_or(1500);
                }
            }
            "-h" | "--help" => {
                println!(
                    "paxi-cli — headless 抓包\n\
                     用法: paxi-cli [-p PORT] [-d DATA_DIR] [-i INTERVAL_MS]\n\
                     Ctrl+C 停止并恢复系统代理"
                );
                return;
            }
            _ => {}
        }
        i += 1;
    }

    std::fs::create_dir_all(&data_dir).expect("创建数据目录失败");
    println!("paxi-cli 数据目录: {}", data_dir.display());

    // 引擎组件（与 GUI 完全一致）
    let ca = CertificateAuthority::load_or_create(&data_dir).expect("CA 初始化失败");
    let db = data_dir.join("traffic.db");
    let store = SqliteStore::open(&db, &data_dir.join("bodies")).expect("存储初始化失败");
    let rules = RulesEngine::open(&db).expect("规则引擎初始化失败");
    let engine = ProxyEngine::new(
        ca,
        store.clone(),
        EventHub::headless(),
        Arc::new(ClientTracker::headless()),
        rules,
    );

    match engine.start(port).await {
        Ok(state) => println!(
            "代理已启动 → http://{}:{port} （{}）",
            state.local_ip,
            if state.running { "运行中" } else { "已停止" }
        ),
        Err(e) => {
            eprintln!("启动失败：{e}");
            std::process::exit(1);
        }
    }

    // 设置系统代理（本机流量进代理）
    match paxi_lib::system_proxy::set_system_proxy(port) {
        Ok(()) => println!("已设置系统代理 :{port}"),
        Err(e) => eprintln!("提示：设置系统代理失败（{e}），可手动配置或仅抓别人/手机的流量"),
    }

    let mut printed = 0usize;
    println!("开始抓包，Ctrl+C 停止…\n");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\n收到 Ctrl+C，停止代理…");
                engine.stop().await;
                // 恢复系统代理（GUI 引擎启动时设置过系统代理）
                let _ = paxi_lib::system_proxy::restore_system_proxy();
                println!("已停止。总共抓到 {} 条记录。", store.list().len());
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(interval_ms)) => {
                let metas = store.list();
                if metas.len() > printed {
                    // 打印新增（最新在前，倒序为正时间序）
                    for m in metas[..metas.len().min(printed + 10)].iter().rev() {
                        let err = m.error.as_deref().map(|e| format!(" err={e}")).unwrap_or_default();
                        let status_str = if m.status == 0 { "✕".to_string() } else { m.status.to_string() };
                        println!(
                            "[{:?}] {} {} — {} {}{}",
                            m.started_at,
                            m.method,
                            m.url,
                            status_str,
                            format!("{}ms", m.duration_ms),
                            err
                        );
                    }
                    printed = metas.len();
                }
            }
        }
    }
}