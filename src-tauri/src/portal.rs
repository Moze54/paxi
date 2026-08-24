//! 门户服务（Portal）：手机扫码/直连代理端口时的引导页与证书分发。
//!
//! 修复 D1 回环 bug：代理对"目标是自身"的请求不再转发（否则会自己转自己），
//! 而是路由到内置门户：
//! - `/`                 门户页（自动识别 iOS / Android / 桌面，展示配置指引）
//! - `/ca.crt` `/ca.pem` 根证书（PEM）
//! - `/ca.der`           根证书（DER）
//! - `/ios.mobileconfig` iOS 描述文件（内嵌 CA）
//! - `/ping`             连接自检 JSON
//! - `/favicon.ico`      空图标

use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Request, Response, Uri};
use std::collections::HashSet;
use std::sync::Arc;

/// 门户服务上下文（代理启动时构建，含最新本机 IP 列表）。
pub struct Portal {
    /// 根证书 PEM
    pub ca_pem: String,
    /// 监听端口
    pub port: u16,
    /// 展示用主 IP（局域网）
    pub display_ip: String,
    /// 本机全部 IP（字符串形式，含 loopback 与 hostname）
    local_hosts: HashSet<String>,
}

impl Portal {
    /// 构建门户：ca_pem 根证书、监听端口、展示 IP。
    pub fn new(ca_pem: String, port: u16, display_ip: String) -> Arc<Self> {
        let mut local_hosts = HashSet::new();
        local_hosts.insert("localhost".to_string());
        local_hosts.insert("127.0.0.1".to_string());
        local_hosts.insert("::1".to_string());
        local_hosts.insert("[::1]".to_string());
        if let Some(hostname) = hostname() {
            local_hosts.insert(hostname.to_lowercase());
        }
        if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
            for (_, ip) in ifaces {
                if ip.is_ipv6() {
                    // IPv6 在 URI host 中带括号，两种形式都收
                    let s = ip.to_string();
                    local_hosts.insert(s.clone());
                    local_hosts.insert(format!("[{s}]"));
                } else {
                    local_hosts.insert(ip.to_string());
                }
            }
        }
        Arc::new(Self {
            ca_pem,
            port,
            display_ip,
            local_hosts,
        })
    }

    /// 该请求是否指向代理自身（应路由到门户）。
    pub fn is_self_target(&self, uri: &Uri, _host_header: Option<&str>) -> bool {
        let uri_host = uri.host().map(|h| h.to_lowercase());
        let Some(host) = uri_host else {
            // origin-form（无 host）：客户端把代理当源站直接访问，一定是门户
            return true;
        };
        if !self.local_hosts.contains(&host) {
            return false;
        }
        match uri.port_u16() {
            Some(p) if p == self.port => true, // 明确指向本代理端口：完全接管
            Some(_) => false,                  // 本机其他端口的服务：正常转发
            None => is_portal_path(uri.path()), // 无端口：仅拦截门户路径
        }
    }
}

/// 机器主机名（小写）。
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.to_lowercase())
}

/// 已知门户路径（用于无端口请求的保守拦截）。
fn is_portal_path(path: &str) -> bool {
    matches!(
        path,
        "/" | "/portal" | "/ca.crt" | "/ca.pem" | "/ca.der" | "/ios.mobileconfig" | "/ping" | "/health" | "/favicon.ico"
    )
}

/// 处理门户请求，返回响应。
pub async fn handle(portal: &Portal, req: Request<hyper::body::Incoming>) -> Response<Full<Bytes>> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();

    if method != Method::GET && method != Method::HEAD {
        return text_response(405, "Method Not Allowed");
    }

    match path.as_str() {
        "/" | "/portal" => html_response(portal_page(portal)),
        "/ca.crt" | "/ca.pem" | "/android.crt" => cert_response(
            portal.ca_pem.clone(),
            "paxi-root-ca.crt",
        ),
        "/ca.der" => match pem_to_der(&portal.ca_pem) {
            Some(der) => Response::builder()
                .status(200)
                .header("Content-Type", "application/x-x509-ca-cert")
                .header("Content-Disposition", "attachment; filename=\"paxi-root-ca.der\"")
                .body(Full::new(Bytes::from(der)))
                .unwrap(),
            None => text_response(500, "证书转换失败"),
        },
        "/ios.mobileconfig" => {
            let config = build_mobileconfig(&portal.ca_pem);
            Response::builder()
                .status(200)
                .header(
                    "Content-Type",
                    "application/x-apple-aspen-config; charset=utf-8",
                )
                .header(
                    "Content-Disposition",
                    "attachment; filename=\"paxi-root-ca.mobileconfig\"",
                )
                .body(Full::new(Bytes::from(config)))
                .unwrap()
        }
        "/ping" | "/health" => {
            let body = format!(
                "{{\"ok\":true,\"proxy\":\"{}:{}\",\"version\":\"0.1.0\"}}",
                portal.display_ip, portal.port
            );
            Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap()
        }
        "/favicon.ico" => Response::builder()
            .status(204)
            .body(Full::new(Bytes::new()))
            .unwrap(),
        _ => text_response(404, "404 Not Found"),
    }
}

fn text_response(status: u16, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Cache-Control", "no-store")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

fn html_response(html: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .body(Full::new(Bytes::from(html)))
        .unwrap()
}

fn cert_response(pem: String, filename: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(200)
        .header("Content-Type", "application/x-x509-ca-cert")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .header("Cache-Control", "no-store")
        .body(Full::new(Bytes::from(pem)))
        .unwrap()
}

/// PEM → DER。
fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

/// 生成 iOS 描述文件（.mobileconfig），内嵌 CA 证书。
fn build_mobileconfig(ca_pem: &str) -> String {
    let der_b64: String = ca_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let payload_uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
    let profile_uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>PayloadContent</key>
    <array>
        <dict>
            <key>PayloadCertificate</key>
            <data>{der_b64}</data>
            <key>PayloadDescription</key>
            <string>paxi 抓包工具根证书</string>
            <key>PayloadDisplayName</key>
            <string>paxi Root CA</string>
            <key>PayloadIdentifier</key>
            <string>com.paxi.proxy.ca</string>
            <key>PayloadType</key>
            <string>com.apple.security.root</string>
            <key>PayloadUUID</key>
            <string>{payload_uuid}</string>
            <key>PayloadVersion</key>
            <integer>1</integer>
        </dict>
    </array>
    <key>PayloadDescription</key>
    <string>安装后即可解密 HTTPS 流量（抓包调试用途）</string>
    <key>PayloadDisplayName</key>
    <string>paxi 根证书</string>
    <key>PayloadIdentifier</key>
    <string>com.paxi.proxy.profile</string>
    <key>PayloadRemovalDisallowed</key>
    <false/>
    <key>PayloadType</key>
    <string>Configuration</string>
    <key>PayloadUUID</key>
    <string>{profile_uuid}</string>
    <key>PayloadVersion</key>
    <integer>1</integer>
</dict>
</plist>"#
    )
}

/// 门户页 HTML。
fn portal_page(portal: &Portal) -> String {
    let proxy_addr = format!("{}:{}", portal.display_ip, portal.port);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>paxi · 证书安装</title>
<style>
:root {{ color-scheme: dark; }}
* {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{
  font-family: -apple-system, "PingFang SC", "Segoe UI", "Microsoft YaHei", sans-serif;
  background: #17181d; color: #d9dae0; line-height: 1.6;
  max-width: 640px; margin: 0 auto; padding: 24px 16px 60px;
}}
.hero {{ text-align: center; margin-bottom: 22px; }}
.logo {{
  width: 56px; height: 56px; border-radius: 14px; margin: 8px auto 10px;
  background: linear-gradient(135deg, #4f8cff, #2ecc71);
  display: flex; align-items: center; justify-content: center;
  font-size: 28px; font-weight: 800; color: #fff;
}}
h1 {{ font-size: 20px; }}
.addr-card {{
  background: #212228; border: 1px solid #35363e; border-radius: 12px;
  padding: 14px 16px; margin-bottom: 18px; text-align: center;
}}
.addr-label {{ font-size: 12px; color: #8b8c94; }}
.addr {{ font-size: 22px; font-weight: 700; color: #4f8cff; letter-spacing: 0.5px; }}
.addr-hint {{ font-size: 12px; color: #8b8c94; margin-top: 4px; }}
.btn {{
  display: block; width: 100%; border: none; border-radius: 10px;
  padding: 13px 16px; font-size: 15px; font-weight: 600; cursor: pointer;
  margin-top: 12px; text-decoration: none; text-align: center;
}}
.btn-primary {{ background: #4f8cff; color: #fff; }}
.btn-ghost {{ background: #2a2b31; color: #d9dae0; border: 1px solid #3a3b42; }}
.section {{ margin-top: 26px; display: none; }}
.section.active {{ display: block; }}
.section-title {{ font-size: 15px; font-weight: 700; margin-bottom: 8px; }}
ol.steps {{ padding-left: 20px; }}
ol.steps li {{ margin: 6px 0; font-size: 14px; }}
ol.steps li code, .addr-card code {{
  background: #2a2b31; padding: 1px 6px; border-radius: 5px; font-size: 13px;
}}
.note {{
  margin-top: 10px; padding: 10px 12px; border-radius: 8px;
  background: #2d2618; border: 1px solid #55461f; font-size: 13px; color: #e8c879;
}}
.footer {{ margin-top: 34px; text-align: center; font-size: 12px; color: #63646c; }}
#check-result {{ margin-top: 10px; font-size: 14px; text-align: center; min-height: 22px; }}
.ok {{ color: #2ecc71; }} .fail {{ color: #e74c3c; }}
</style>
</head>
<body>
<div class="hero">
  <div class="logo">P</div>
  <h1>paxi 证书安装</h1>
</div>

<div class="addr-card">
  <div class="addr-label">代理服务器地址（在手机 Wi-Fi 代理设置中填写）</div>
  <div class="addr">{proxy_addr}</div>
  <div class="addr-hint">服务器：<b>{ip}</b> · 端口：<b>{port}</b></div>
</div>

<a class="btn btn-primary" href="/ca.crt" download>下载根证书（.crt）</a>

<div id="sec-ios" class="section">
  <div class="section-title">iOS 安装步骤</div>
  <ol class="steps">
    <li>点击上方按钮，或改用 <a href="/ios.mobileconfig" style="color:#4f8cff">描述文件方式安装</a>（推荐）</li>
    <li>允许下载配置 → 打开<b>设置</b>，顶部出现<b>已下载描述文件</b> → 安装</li>
    <li><b>关键一步</b>：设置 → 通用 → 关于本机 → 证书信任设置 → 打开 <code>paxi Root CA</code> 的完全信任开关</li>
  </ol>
</div>

<div id="sec-android" class="section">
  <div class="section-title">Android 安装步骤</div>
  <ol class="steps">
    <li>点击上方按钮下载 <code>paxi-root-ca.crt</code></li>
    <li>打开<b>设置 → 安全 → 加密与凭据 → 安装证书 → CA 证书</b>（不同机型路径略有差异，可搜索"证书"）</li>
    <li>选择下载的证书文件安装（可能提示风险，属正常）</li>
  </ol>
  <div class="note">Android 7+ 默认不信任用户证书，App 抓包需在应用的 networkSecurityConfig 中放行，或使用 Android 6 及以下设备 / 模拟器。浏览器抓包不受影响。</div>
</div>

<div id="sec-desktop" class="section">
  <div class="section-title">桌面系统安装步骤</div>
  <ol class="steps">
    <li>下载证书后双击打开</li>
    <li>Windows：安装到<b>本地计算机 → 受信任的根证书颁发机构</b></li>
    <li>macOS：钥匙串访问 → 系统 → 导入 → 设为<b>始终信任</b></li>
  </ol>
</div>

<button class="btn btn-ghost" onclick="runCheck()">我已经配好代理，测试连接</button>
<div id="check-result"></div>

<div class="footer">paxi · 开发者抓包工具 · 仅供调试用途</div>

<script>
(function() {{
  var ua = navigator.userAgent;
  var isIOS = /iPhone|iPad|iPod/i.test(ua);
  var isAndroid = /Android/i.test(ua);
  var id = isIOS ? 'sec-ios' : (isAndroid ? 'sec-android' : 'sec-desktop');
  var el = document.getElementById(id);
  if (el) el.classList.add('active');
}})();

function runCheck() {{
  var el = document.getElementById('check-result');
  el.textContent = '检测中…';
  el.className = '';
  fetch('/ping')
    .then(function(r) {{ return r.json(); }})
    .then(function(d) {{
      el.textContent = '✓ 代理工作正常（' + d.proxy + '）';
      el.className = 'ok';
    }})
    .catch(function() {{
      el.textContent = '✗ 无法连接门户，请确认手机与电脑在同一网络';
      el.className = 'fail';
    }});
}}
</script>
</body>
</html>"#,
        proxy_addr = proxy_addr,
        ip = portal.display_ip,
        port = portal.port,
    )
}
