//! 配对二维码解析与校验。
//!
//! 二维码格式（以 Windows 服务端实际输出为准）：
//! `{type:"ltd-pair", v:1, protocol:"1", host, port, fingerprint, code, expiresAt}`
//!
//! 只接受家庭局域网 IPv4 地址，拒绝任意 URL/域名与公网地址；配对码、指纹、
//! 有效期都会在发起任何网络请求前完成校验。

use std::net::Ipv4Addr;

use serde::Serialize;
use serde_json::Value;

use super::pinned::{format_fingerprint, normalize_fingerprint};
use super::PROTOCOL_VERSION;

#[derive(Debug, Clone)]
pub struct PairingPayload {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    /// 规范化的大写冒号分隔指纹
    pub fingerprint: String,
    pub code: String,
    pub expires_at: i64,
}

/// 扫码后展示给用户的服务器信息（尚未配对）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPreview {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub expires_at: i64,
    pub protocol_version: String,
    pub expired: bool,
}

#[derive(Debug, Clone)]
pub struct PairingError(pub String);

impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PairingError {}

fn fail<T>(msg: impl Into<String>) -> Result<T, PairingError> {
    Err(PairingError(msg.into()))
}

fn parse_fields(json: &str) -> Result<PairingPayload, PairingError> {
    let value: Value = serde_json::from_str(json)
        .map_err(|_| PairingError("二维码内容不是有效的配对信息".into()))?;
    let obj = value
        .as_object()
        .ok_or_else(|| PairingError("二维码内容不是有效的配对信息".into()))?;

    let kind = obj.get("type").and_then(Value::as_str).unwrap_or_default();
    if kind != "ltd-pair" {
        return fail("不是 Local To Do 的配对二维码");
    }
    let version = obj.get("v").and_then(Value::as_u64).unwrap_or_default();
    if version != 1 {
        return fail("配对二维码版本不受支持");
    }
    let protocol = obj
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if protocol != PROTOCOL_VERSION {
        return fail(format!(
            "服务器同步协议版本不受支持（{protocol}），请更新应用"
        ));
    }

    let host = obj
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let ip: Ipv4Addr = host
        .parse()
        .map_err(|_| PairingError("配对二维码中的服务器地址无效".into()))?;
    if !(ip.is_private() || ip.is_loopback() || ip.is_link_local()) {
        return fail("配对二维码中的服务器地址不是家庭局域网地址，已拒绝");
    }

    let port = obj
        .get("port")
        .and_then(Value::as_u64)
        .filter(|p| (1..=65535).contains(p))
        .ok_or_else(|| PairingError("配对二维码中的端口无效".into()))? as u16;

    let fingerprint_text = obj
        .get("fingerprint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let fingerprint_bytes = normalize_fingerprint(fingerprint_text)
        .map_err(|e| PairingError(e.to_string()))?;

    let code = obj
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if code.len() != 8 || !code.chars().all(|c| c.is_ascii_digit()) {
        return fail("配对二维码中的配对码无效");
    }

    let expires_at = obj
        .get("expiresAt")
        .and_then(Value::as_i64)
        .filter(|v| *v > 0)
        .ok_or_else(|| PairingError("配对二维码缺少有效期".into()))?;

    Ok(PairingPayload {
        protocol: protocol.to_string(),
        host: ip.to_string(),
        port,
        fingerprint: format_fingerprint(&fingerprint_bytes),
        code,
        expires_at,
    })
}

/// 严格解析：结构非法或已过期都会失败。
pub fn parse_payload(json: &str, now_ms: i64) -> Result<PairingPayload, PairingError> {
    let payload = parse_fields(json)?;
    if now_ms > payload.expires_at {
        return fail("配对二维码已过期，请在电脑上重新生成");
    }
    Ok(payload)
}

/// 宽松解析：仅校验结构，返回是否过期（用于扫码后展示服务器信息）。
pub fn preview(json: &str, now_ms: i64) -> Result<PairingPreview, PairingError> {
    let payload = parse_fields(json)?;
    Ok(PairingPreview {
        host: payload.host,
        port: payload.port,
        fingerprint: payload.fingerprint,
        expires_at: payload.expires_at,
        protocol_version: payload.protocol,
        expired: now_ms > payload.expires_at,
    })
}
