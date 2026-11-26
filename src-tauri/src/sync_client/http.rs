//! 基于 rustls 的最小 HTTPS/1.1 客户端。
//!
//! 只实现同步接口所需的 GET/POST JSON 请求，避免引入额外 HTTP 依赖；
//! 每次连接都使用 `pinned` 模块构建的指纹锁定配置，因此所有请求都执行
//! 相同的证书指纹校验。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use rustls::pki_types::ServerName;
use rustls::ClientConfig;

use super::pinned::{client_config_for, CERT_PIN_MISMATCH};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// 单次响应体上限，避免异常服务端耗尽手机内存
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum ClientError {
    /// TCP 连接失败（服务未启动、手机与电脑不在同一网络等）
    Connect(String),
    /// 证书指纹与配对信息不一致
    TlsPin(String),
    /// 其他 TLS 握手错误
    Tls(String),
    /// 超时
    Timeout(String),
    /// 读写失败
    Io(String),
    /// 响应格式不符合 HTTP/JSON
    Protocol(String),
}

impl ClientError {
    pub fn message(&self) -> &str {
        match self {
            ClientError::Connect(m)
            | ClientError::TlsPin(m)
            | ClientError::Tls(m)
            | ClientError::Timeout(m)
            | ClientError::Io(m)
            | ClientError::Protocol(m) => m,
        }
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// HTTP 响应（状态码 + 已解析的 JSON，空响应为 Null）
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Value,
}

/// 指纹锁定的 HTTPS 客户端（每次请求重新连接并重新校验证书）
pub struct PinnedHttpClient {
    addr: SocketAddr,
    config: Arc<ClientConfig>,
    server_name: ServerName<'static>,
}

impl PinnedHttpClient {
    pub fn new(host: &str, port: u16, fingerprint: &str) -> Result<Self, ClientError> {
        let ip = host
            .parse::<std::net::IpAddr>()
            .map_err(|_| ClientError::Protocol("服务器地址必须是 IP 地址".into()))?;
        let (config, server_name) = client_config_for(host, fingerprint)
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        Ok(Self {
            addr: SocketAddr::new(ip, port),
            config,
            server_name,
        })
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<HttpResponse, ClientError> {
        let tcp = match timeout(CONNECT_TIMEOUT, TcpStream::connect(self.addr)).await {
            Err(_) => {
                return Err(ClientError::Timeout(format!(
                    "连接 {} 超时，请确认手机与电脑在同一 Wi-Fi",
                    self.addr
                )))
            }
            Ok(Err(e)) => {
                return Err(ClientError::Connect(format!(
                    "无法连接 {}：{e}",
                    self.addr
                )))
            }
            Ok(Ok(stream)) => stream,
        };
        let _ = tcp.set_nodelay(true);

        let connector = TlsConnector::from(self.config.clone());
        let mut tls = match timeout(
            REQUEST_TIMEOUT,
            connector.connect(self.server_name.clone(), tcp),
        )
        .await
        {
            Err(_) => return Err(ClientError::Timeout("HTTPS 握手超时".into())),
            Ok(Err(e)) => {
                let text = e.to_string();
                if text.contains(CERT_PIN_MISMATCH) {
                    return Err(ClientError::TlsPin(
                        "服务器证书指纹与配对时不一致，已终止连接".into(),
                    ));
                }
                return Err(ClientError::Tls(format!("HTTPS 握手失败：{text}")));
            }
            Ok(Ok(stream)) => stream,
        };

        let body_bytes = body.map(|v| v.to_string().into_bytes()).unwrap_or_default();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n",
            self.addr
        );
        if let Some(token) = token {
            head.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        if body.is_some() {
            head.push_str("Content-Type: application/json\r\n");
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", body_bytes.len()));

        let io = async {
            tls.write_all(head.as_bytes()).await?;
            if !body_bytes.is_empty() {
                tls.write_all(&body_bytes).await?;
            }
            tls.flush().await?;
            let mut raw = Vec::new();
            let mut limited = tls.take(MAX_RESPONSE_BYTES as u64 + 1);
            limited.read_to_end(&mut raw).await?;
            Ok::<Vec<u8>, std::io::Error>(raw)
        };

        let raw = match timeout(REQUEST_TIMEOUT, io).await {
            Err(_) => return Err(ClientError::Timeout("等待服务器响应超时".into())),
            Ok(Err(e)) => return Err(ClientError::Io(format!("HTTPS 读写失败：{e}"))),
            Ok(Ok(raw)) => raw,
        };
        if raw.len() > MAX_RESPONSE_BYTES {
            return Err(ClientError::Protocol("服务器响应过大，已终止".into()));
        }
        parse_http_response(&raw)
    }
}

/// 解析 HTTP/1.1 响应（支持 Content-Length 与 chunked）
pub(crate) fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, ClientError> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| ClientError::Protocol("HTTP 响应不完整".into()))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|_| ClientError::Protocol("HTTP 响应头不是 UTF-8".into()))?;

    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| ClientError::Protocol("HTTP 响应缺少状态码".into()))?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                content_length = value.parse::<usize>().ok();
            } else if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
        }
    }

    let body_raw = &raw[split + 4..];
    let body = if chunked {
        decode_chunked(body_raw)?
    } else if let Some(len) = content_length {
        if body_raw.len() < len {
            return Err(ClientError::Protocol("HTTP 响应体不完整".into()));
        }
        body_raw[..len].to_vec()
    } else {
        body_raw.to_vec()
    };

    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| ClientError::Protocol(format!("服务器响应不是有效 JSON：{e}")))?
    };
    Ok(HttpResponse { status, body })
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, ClientError> {
    let mut out = Vec::new();
    loop {
        let end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| ClientError::Protocol("分块响应不完整".into()))?;
        let line = std::str::from_utf8(&data[..end])
            .map_err(|_| ClientError::Protocol("分块长度无效".into()))?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| ClientError::Protocol("分块长度无效".into()))?;
        data = &data[end + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size + 2 {
            return Err(ClientError::Protocol("分块数据不完整".into()));
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"ok\":true}  ";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body["ok"], serde_json::json!(true));
    }

    #[test]
    fn parses_chunked_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.body["a"], serde_json::json!(1));
    }
}
