//! CA 证书管理：生成自签根证书、按域名动态签发叶子证书、导出证书。
//!
//! HTTPS 中间人的原理：
//! 1. 首次运行生成一个自签 CA 根证书，用户把它安装并信任到系统/手机。
//! 2. 当代理收到 CONNECT example.com:443 时，用 CA 为 example.com 现场签发一张叶子证书。
//! 3. 代理用这张叶子证书与客户端完成 TLS 握手，从而能解密客户端发出的 HTTPS 内容。
//! 4. 代理再以普通客户端身份连接真实的 example.com 服务器。

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
    SanType,
};
use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use time::{Duration, OffsetDateTime};

/// CA 证书管理器的内部状态。
pub struct CertificateAuthority {
    /// 根证书（含私钥），用于签发叶子证书。
    root_cert: Certificate,
    /// 根证书的私钥（以 PEM 保存，便于导出）。
    root_key_pem: String,
    /// 根证书的 PEM 文本。
    root_cert_pem: String,
    /// 已签发的叶子证书缓存：域名 -> (证书 PEM, 私钥 PEM)。
    /// PEM 文本来自 rcgen 的 serialize 方法，可直接用于 rustls。
    cache: Mutex<std::collections::HashMap<String, (String, String)>>,
}

impl CertificateAuthority {
    /// 从磁盘加载已存在的 CA（若存在），否则新建一个。
    pub fn load_or_create(dir: &PathBuf) -> Result<Arc<Self>, String> {
        let cert_path = dir.join("ca-cert.pem");
        let key_path = dir.join("ca-key.pem");

        if cert_path.exists() && key_path.exists() {
            let cert_pem = fs::read_to_string(&cert_path).map_err(|e| e.to_string())?;
            let key_pem = fs::read_to_string(&key_path).map_err(|e| e.to_string())?;
            let key_pair = KeyPair::from_pem(&key_pem).map_err(|e| e.to_string())?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .map_err(|e| e.to_string())?;
            let root_cert = params
                .self_signed(&key_pair)
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(Self {
                root_cert,
                root_key_pem: key_pem,
                root_cert_pem: cert_pem,
                cache: Mutex::new(std::collections::HashMap::new()),
            }))
        } else {
            let ca = Self::create_new()?;
            // 持久化
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            fs::write(&cert_path, &ca.root_cert_pem).map_err(|e| e.to_string())?;
            fs::write(&key_path, &ca.root_key_pem).map_err(|e| e.to_string())?;
            Ok(Arc::new(ca))
        }
    }

    /// 新建一个自签 CA。
    fn create_new() -> Result<Self, String> {
        let mut params = CertificateParams::new(vec![]).map_err(|e| e.to_string())?;
        params
            .distinguished_name
            .push(DnType::CommonName, "paxi-proxy CA");
        params
            .distinguished_name
            .push(DnType::OrganizationName, "paxi");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        // 根证书有效期 10 年
        params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(3650);

        let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
        let root_cert = params.self_signed(&key_pair).map_err(|e| e.to_string())?;

        let root_cert_pem = root_cert.pem();
        let root_key_pem = key_pair.serialize_pem();

        Ok(Self {
            root_cert,
            root_key_pem,
            root_cert_pem,
            cache: Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// 为指定域名签发一张叶子证书，返回 (证书 PEM, 私钥 PEM)。
    /// 带缓存，同一域名只签发一次。
    pub fn leaf_for_host(&self, host: &str) -> Result<(String, String), String> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some(hit) = cache.get(host) {
                return Ok(hit.clone());
            }
        }

        // SAN：IP 直连（如微信 mars 框架）用 IpAddress 类型，域名用 DnsName
        let mut params = if let Ok(ip) = host.parse::<IpAddr>() {
            let mut p = CertificateParams::new(vec![]).map_err(|e| e.to_string())?;
            p.subject_alt_names.push(SanType::IpAddress(ip));
            p.distinguished_name.push(DnType::CommonName, host.to_string());
            p
        } else {
            let mut p = CertificateParams::new(vec![host.to_string()])
                .map_err(|e| e.to_string())?;
            p.distinguished_name.push(DnType::CommonName, host.to_string());
            p
        };
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(825);

        let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
        let leaf_cert = params
            .signed_by(&key_pair, &self.root_cert, &self.root_key_pair())
            .map_err(|e| e.to_string())?;

        let cert_pem = leaf_cert.pem();
        let key_pem = key_pair.serialize_pem();

        let mut cache = self.cache.lock().unwrap();
        cache.insert(host.to_string(), (cert_pem.clone(), key_pem.clone()));
        Ok((cert_pem, key_pem))
    }

    /// 返回根证书 PEM 文本。
    pub fn root_cert_pem(&self) -> String {
        self.root_cert_pem.clone()
    }

    /// 内部：从 PEM 重建根证书私钥以便签发。
    fn root_key_pair(&self) -> KeyPair {
        KeyPair::from_pem(&self.root_key_pem).expect("root key should be valid")
    }

    /// 导出根证书到指定文件路径（默认导出 .crt，兼容 Windows 双击安装）。
    pub fn export_root_cert(&self, path: &PathBuf) -> Result<(), String> {
        // 也顺带导出 DER 格式，方便手机安装（可选，这里以 PEM 的 .crt 为主）
        fs::write(path, &self.root_cert_pem).map_err(|e| e.to_string())?;
        Ok(())
    }
}
