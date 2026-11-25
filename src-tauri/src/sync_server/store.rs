//! 已配对设备与配对码状态。
//!
//! 设备信息以独立 JSON 文件保存在应用数据目录（不修改数据库 schema）：
//! - 只保存设备令牌的 SHA-256 哈希，明文令牌只在配对成功时返回一次；
//! - 比对使用常量时间比较，避免时序侧信道；
//! - 撤销设备后立即从内存与文件中移除，其令牌随即失效。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use ltd_core::error::{AppError, AppResult};

const FILE_VERSION: u32 = 1;
const LAST_SEEN_PERSIST_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceRecord {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub created_at: i64,
    #[serde(default)]
    pub last_seen_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceFile {
    pub version: u32,
    #[serde(default)]
    pub devices: Vec<DeviceRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
}

impl From<&DeviceRecord> for DeviceView {
    fn from(r: &DeviceRecord) -> Self {
        Self {
            id: r.id.clone(),
            name: r.name.clone(),
            created_at: r.created_at,
            last_seen_at: r.last_seen_at,
        }
    }
}

struct Inner {
    loaded: bool,
    devices: Vec<DeviceRecord>,
    last_saved: Option<Instant>,
}

pub struct DeviceStore {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl DeviceStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            inner: Mutex::new(Inner {
                loaded: false,
                devices: Vec::new(),
                last_saved: None,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> AppResult<Vec<DeviceView>> {
        let mut inner = self.lock();
        self.ensure_loaded(&mut inner)?;
        Ok(inner.devices.iter().map(DeviceView::from).collect())
    }

    pub fn count(&self) -> AppResult<usize> {
        let mut inner = self.lock();
        self.ensure_loaded(&mut inner)?;
        Ok(inner.devices.len())
    }

    /// 新设备登记（token_hash 为令牌的 SHA-256）
    pub fn add(&self, name: &str, token_hash: &str, now: i64) -> AppResult<DeviceView> {
        let mut inner = self.lock();
        self.ensure_loaded(&mut inner)?;
        let record = DeviceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            token_hash: token_hash.to_string(),
            created_at: now,
            last_seen_at: None,
        };
        inner.devices.push(record.clone());
        self.save(&mut inner)?;
        Ok(DeviceView::from(&record))
    }

    /// 校验令牌。命中后更新最后访问时间（最多每 5 分钟落盘一次）。
    pub fn authorize(&self, token: &str, now: i64) -> AppResult<Option<DeviceView>> {
        let hash = hash_token(token);
        let mut inner = self.lock();
        self.ensure_loaded(&mut inner)?;

        let mut found: Option<usize> = None;
        for (i, d) in inner.devices.iter().enumerate() {
            let eq: bool = d
                .token_hash
                .as_bytes()
                .ct_eq(hash.as_bytes())
                .into();
            if eq {
                found.get_or_insert(i);
            }
        }

        let Some(index) = found else {
            return Ok(None);
        };
        inner.devices[index].last_seen_at = Some(now);
        let should_persist = inner
            .last_saved
            .map(|t| t.elapsed() >= LAST_SEEN_PERSIST_INTERVAL)
            .unwrap_or(true);
        if should_persist {
            // 最后访问时间写入失败不影响本次鉴权
            let _ = self.save(&mut inner);
        }
        Ok(Some(DeviceView::from(&inner.devices[index])))
    }

    pub fn revoke(&self, id: &str) -> AppResult<bool> {
        let mut inner = self.lock();
        self.ensure_loaded(&mut inner)?;
        let before = inner.devices.len();
        inner.devices.retain(|d| d.id != id);
        if inner.devices.len() == before {
            return Ok(false);
        }
        self.save(&mut inner)?;
        Ok(true)
    }

    pub fn clear(&self) -> AppResult<()> {
        let mut inner = self.lock();
        self.ensure_loaded(&mut inner)?;
        inner.devices.clear();
        self.save(&mut inner)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ensure_loaded(&self, inner: &mut Inner) -> AppResult<()> {
        if inner.loaded {
            return Ok(());
        }
        match fs::read(&self.path) {
            Ok(bytes) => {
                let file: DeviceFile = serde_json::from_slice(&bytes).map_err(|e| {
                    AppError::Other(format!(
                        "已配对设备文件无法解析（{}）：{e}",
                        self.path.display()
                    ))
                })?;
                inner.devices = file.devices;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        inner.loaded = true;
        Ok(())
    }

    fn save(&self, inner: &mut Inner) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = DeviceFile {
            version: FILE_VERSION,
            devices: inner.devices.clone(),
        };
        let text = serde_json::to_vec_pretty(&file)?;
        fs::write(&self.path, text)?;
        inner.last_saved = Some(Instant::now());
        Ok(())
    }
}

/// 设备令牌的 SHA-256 哈希（小写十六进制）。
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
