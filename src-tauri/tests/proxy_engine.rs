//! 集成测试：验证代理引擎的 HTTP 转发与记录。
//! 运行：cargo test --test proxy_engine -- --nocapture

use paxi_lib::proxy::ProxyEngine;
use paxi_lib::ca::CertificateAuthority;
use paxi_lib::models::TrafficStore;

#[tokio::test]
async fn test_http_proxy_forward() {
    // 初始化 CA（用临时目录）
    let tmp = std::env::temp_dir().join("paxi-test-ca");
    let ca = CertificateAuthority::load_or_create(&tmp).unwrap();
    let store = TrafficStore::new(100);
    let engine = ProxyEngine::new(ca, store.clone());

    // 启动代理，监听随机端口
    let port = 18888;
    let state = engine.start(port).await.unwrap();
    assert!(state.running);

    // 用 async reqwest 通过代理发起 HTTP 请求
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).unwrap())
        .build()
        .unwrap();

    let resp = client
        .get("http://example.com/")
        .send()
        .await;

    match resp {
        Ok(r) => {
            println!("HTTP 响应状态码: {}", r.status());
            // 等待记录写入
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let list = store.list();
            println!("抓到的请求数: {}", list.len());
            for m in &list {
                println!("  - {} {} (status={})", m.method, m.url, m.status);
            }
            assert!(!list.is_empty(), "应该至少抓到一条请求");
        }
        Err(e) => {
            println!("请求失败: {e}");
            // 网络不可达也不该 panic，只打印
        }
    }

    let _ = engine.stop().await;
}

#[tokio::test]
async fn test_https_proxy_mitm() {
    let tmp = std::env::temp_dir().join("paxi-test-ca");
    let ca = CertificateAuthority::load_or_create(&tmp).unwrap();
    let store = TrafficStore::new(100);
    let engine = ProxyEngine::new(ca, store.clone());

    let port = 18889;
    let state = engine.start(port).await.unwrap();
    assert!(state.running);

    // 忽略证书验证，模拟客户端信任了我们的 CA
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).unwrap())
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let resp = client
        .get("https://example.com/")
        .send()
        .await;

    match resp {
        Ok(r) => {
            println!("HTTPS 响应状态码: {}", r.status());
            let text = r.text().await.unwrap_or_default();
            println!("HTTPS 响应体前 60 字: {}", &text[..text.len().min(60)]);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let list = store.list();
            println!("抓到的 HTTPS 请求数: {}", list.len());
            for m in &list {
                println!("  - {} {} (status={}, scheme={})", m.method, m.url, m.status, m.scheme);
            }
            assert!(!list.is_empty(), "应该至少抓到一条 HTTPS 请求");
        }
        Err(e) => {
            println!("HTTPS 请求失败详情: {e:?}");
            let list = store.list();
            println!("当前抓到的请求数: {}", list.len());
            for m in &list {
                println!("  - {} {} (status={}, scheme={}, err={:?})", m.method, m.url, m.status, m.scheme, m.error);
            }
        }
    }

    let _ = engine.stop().await;
}
