use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ltd_core::db::Db;

use crate::sync_client::{default_store, MobileSyncService};
use crate::sync_server::SyncService;

/// 全局应用状态（由 Tauri 管理）
pub struct AppState {
    pub db: Arc<Db>,
    pub data_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub log_dir: PathBuf,
    /// 旧局域网同步服务；独立 Android 版为 None，不创建目录、证书或镜像库。
    pub sync: Option<SyncService>,
    /// 旧 companion 同步客户端；仅非 Android 构建保留给历史代码与测试。
    pub mobile_sync: Option<Arc<MobileSyncService>>,
    /// 调度器唤醒信号：Mutex<bool> + Condvar
    pub wake: Arc<(Mutex<bool>, Condvar)>,
}

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf, backup_dir: PathBuf, log_dir: PathBuf) -> Self {
        let db = Arc::new(db);
        #[cfg(not(target_os = "android"))]
        let (sync, mobile_sync) = {
            let sync = SyncService::new(db.clone(), data_dir.join("sync"));
            let mobile_dir = data_dir.join("mobile-sync");
            let mobile_sync = Arc::new(MobileSyncService::new(
                mobile_dir.clone(),
                default_store(&mobile_dir),
            ));
            (Some(sync), Some(mobile_sync))
        };
        #[cfg(target_os = "android")]
        let (sync, mobile_sync) = (None, None);
        Self {
            db,
            data_dir,
            backup_dir,
            log_dir,
            sync,
            mobile_sync,
            wake: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    #[allow(dead_code)]
    pub fn sync(&self) -> &SyncService {
        self.sync.as_ref().expect("旧同步服务仅在非 Android 端可用")
    }

    #[allow(dead_code)]
    pub fn mobile_sync(&self) -> &Arc<MobileSyncService> {
        self.mobile_sync
            .as_ref()
            .expect("旧 companion 同步客户端仅在非 Android 端可用")
    }

    /// 数据变化后立即唤醒调度器重新计算
    pub fn notify_scheduler(&self) {
        let (lock, cv) = &*self.wake;
        let mut flag = lock.lock().unwrap_or_else(|e| e.into_inner());
        *flag = true;
        cv.notify_all();
        drop(flag);

        #[cfg(mobile)]
        {
            let status = crate::sync_client::commands::resync_from_db(self);
            if let Some(error) = status.error {
                crate::logging::warn(format!("Android 提醒重同步失败：{error}"));
            }
        }
    }

    /// 等待唤醒信号或超时
    pub fn wait_for_wake(&self, timeout: Duration) {
        let (lock, cv) = &*self.wake;
        let mut flag = lock.lock().unwrap_or_else(|e| e.into_inner());
        if !*flag {
            let (f, _res) = cv
                .wait_timeout(flag, timeout)
                .unwrap_or_else(|e| e.into_inner());
            flag = f;
        }
        *flag = false;
    }
}
