//! HTTPS 证书指纹锁定。
//!
//! Windows 同步服务使用自签名证书，其 SAN 只包含生成时的局域网 IP，IP 变化后
//! 常规主机名校验会误报。这里改用「证书指纹锁定」：
//! - 只接受叶子证书 DER 的 SHA-256 与二维码中指纹完全一致的连接；
//! - 不做主机名校验（指纹已经唯一确定该证书）；
//! - 签名算法验证仍委托 rustls 的 ring provider，不关闭 TLS 校验流程。
//!
//! 安全边界：攻击者必须持有该证书对应的私钥才能通过校验；由于指纹在配对前
//! 已由二维码（可信信道）传入，中间人无法替换证书。

use std::sync::Arc;

use rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use ltd_core::error::{err, AppError, AppResult};

/// 指纹不匹配时写入 TLS 错误的标记，便于上层区分「证书变化」与其他 TLS 故障
pub const CERT_PIN_MISMATCH: &str = "LTD_CERT_PIN_MISMATCH";

/// 规范化指纹：忽略冒号/空格与大小写，要求恰好 64 个十六进制字符。
pub fn normalize_fingerprint(text: &str) -> AppResult<[u8; 32]> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    if cleaned.len() != 64 || !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return err("证书指纹格式无效，应为 64 位十六进制");
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&cleaned[i * 2..i * 2 + 2], 16)
            .map_err(|_| AppError::Other("证书指纹格式无效".into()))?;
    }
    Ok(out)
}

/// 规范展示形式：大写、冒号分隔（与服务端一致）。
pub fn format_fingerprint(bytes: &[u8; 32]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// 测量证书 DER 的 SHA-256。
pub fn fingerprint_of_der(der: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(der);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

/// 只做指纹比较的证书校验器。
#[derive(Debug)]
struct PinnedVerifier {
    pin: [u8; 32],
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let actual = fingerprint_of_der(end_entity.as_ref());
        if !bool::from(actual.ct_eq(&self.pin)) {
            return Err(TlsError::General(CERT_PIN_MISMATCH.to_string()));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 构建只信任指定证书指纹的客户端配置。
pub fn build_pinned_config(pin: [u8; 32]) -> AppResult<Arc<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| AppError::Other(format!("初始化 TLS 失败: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier { pin, provider }))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// 为指定主机与指纹构建客户端配置，同时给出 TLS 的 ServerName。
/// 主机名不参与证书校验，仅用于填充 TLS 握手参数。
pub fn client_config_for(
    host: &str,
    fingerprint: &str,
) -> AppResult<(Arc<ClientConfig>, ServerName<'static>)> {
    let pin = normalize_fingerprint(fingerprint)?;
    let config = build_pinned_config(pin)?;
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| AppError::Other("服务器地址无效".into()))?;
    Ok((config, server_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_client::pinned::format_fingerprint;

    #[test]
    fn fingerprint_normalization() {
        let raw = "aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99:aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99";
        let bytes = normalize_fingerprint(raw).unwrap();
        assert_eq!(
            format_fingerprint(&bytes),
            "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"
        );
        assert!(normalize_fingerprint("").is_err());
        assert!(normalize_fingerprint(&"z".repeat(64)).is_err());
    }
}
