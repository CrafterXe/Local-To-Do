//! 局域网同步服务的 TLS 证书管理。
//!
//! 采用「本地自签名 CA + 服务器证书」：
//! - CA 私钥与服务器私钥只保存在应用数据目录，任何接口都不会返回私钥；
//! - Android 端通过二维码取得服务器证书的 SHA-256 指纹并做证书锁定，
//!   因此证书更换意味着必须重新配对（见 `SyncService::reset_pairing`）。
//! - 证书私钥不写入日志。

use std::fs::{self, File};
use std::io::BufReader;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::CertificateDer;
use rustls::ServerConfig;
use sha2::{Digest, Sha256};

use ltd_core::error::{err, AppError, AppResult};

const CA_CERT_FILE: &str = "ca.crt";
const CA_KEY_FILE: &str = "ca.key";
const SERVER_CERT_FILE: &str = "server.crt";
const SERVER_KEY_FILE: &str = "server.key";

/// 已就绪的 TLS 材料。`fingerprint` 为服务器证书的 SHA-256 指纹（大写、冒号分隔）。
pub struct TlsMaterial {
    pub config: Arc<ServerConfig>,
    pub fingerprint: String,
    /// 本次调用是否重新生成了证书
    pub created: bool,
}

/// 读取已有证书；若证书文件尚不存在则生成一套新的。
///
/// 只有当四个文件全部缺失时才会生成；部分缺失视为异常，避免在用户不知情时
/// 更换证书导致已配对设备全部失效。
pub fn load_or_create(dir: &Path, bind_ip: IpAddr) -> AppResult<TlsMaterial> {
    let ca_cert = dir.join(CA_CERT_FILE);
    let ca_key = dir.join(CA_KEY_FILE);
    let server_cert = dir.join(SERVER_CERT_FILE);
    let server_key = dir.join(SERVER_KEY_FILE);

    let all_exist = ca_cert.is_file() && ca_key.is_file() && server_cert.is_file() && server_key.is_file();
    let none_exist =
        !ca_cert.exists() && !ca_key.exists() && !server_cert.exists() && !server_key.exists();

    let created = if all_exist {
        false
    } else if none_exist {
        generate(dir, bind_ip)?;
        true
    } else {
        return err(format!(
            "同步证书文件不完整（{}），为避免已配对设备失效已停止启动。请使用「重置配对」重新生成证书。",
            dir.display()
        ));
    };

    let fingerprint = fingerprint_of(&server_cert)?;
    let config = build_server_config(&server_cert, &server_key)?;

    Ok(TlsMaterial {
        config,
        fingerprint,
        created,
    })
}

/// 删除全部证书文件（用于「重置配对」）。
pub fn remove_all(dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    for name in [CA_CERT_FILE, CA_KEY_FILE, SERVER_CERT_FILE, SERVER_KEY_FILE] {
        let path = dir.join(name);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn generate(dir: &Path, bind_ip: IpAddr) -> AppResult<()> {
    fs::create_dir_all(dir)?;

    // ---- CA ----
    let ca_key = KeyPair::generate().map_err(cert_err)?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Local To Do Sync CA");
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca_cert = ca_params.self_signed(&ca_key).map_err(cert_err)?;

    // ---- 服务器证书 ----
    let server_key = KeyPair::generate().map_err(cert_err)?;
    let mut sans = vec!["localhost".to_string(), Ipv4Addr::LOCALHOST.to_string()];
    let ip_text = bind_ip.to_string();
    if !sans.contains(&ip_text) {
        sans.push(ip_text);
    }
    let mut server_params = CertificateParams::new(sans).map_err(cert_err)?;
    server_params
        .distinguished_name
        .push(DnType::CommonName, "Local To Do Sync Server");
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    server_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .map_err(cert_err)?;

    fs::write(dir.join(CA_CERT_FILE), ca_cert.pem())?;
    fs::write(dir.join(CA_KEY_FILE), ca_key.serialize_pem())?;
    fs::write(dir.join(SERVER_CERT_FILE), server_cert.pem())?;
    fs::write(dir.join(SERVER_KEY_FILE), server_key.serialize_pem())?;
    Ok(())
}

fn build_server_config(cert_path: &Path, key_path: &Path) -> AppResult<Arc<ServerConfig>> {
    let certs: Vec<CertificateDer<'static>> = {
        let mut reader = BufReader::new(File::open(cert_path)?);
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?
    };
    if certs.is_empty() {
        return err(format!("同步证书为空：{}", cert_path.display()));
    }
    let key = {
        let mut reader = BufReader::new(File::open(key_path)?);
        rustls_pemfile::private_key(&mut reader)?
            .ok_or_else(|| AppError::Other("同步证书私钥文件无效".into()))?
    };

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_err)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(tls_err)?;
    Ok(Arc::new(config))
}

fn fingerprint_of(cert_path: &Path) -> AppResult<String> {
    let certs: Vec<CertificateDer<'static>> = {
        let mut reader = BufReader::new(File::open(cert_path)?);
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?
    };
    let der = certs
        .first()
        .ok_or_else(|| AppError::Other("同步证书为空".into()))?;
    Ok(fingerprint_sha256(der))
}

/// SHA-256 指纹，大写十六进制、冒号分隔（例如 `AB:CD:...`）。
pub fn fingerprint_sha256(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn cert_err(e: rcgen::Error) -> AppError {
    AppError::Other(format!("生成同步证书失败: {e}"))
}

fn tls_err(e: rustls::Error) -> AppError {
    AppError::Other(format!("配置 TLS 失败: {e}"))
}
