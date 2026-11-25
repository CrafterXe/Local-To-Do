use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ltd_core::db::Db;

use crate::sync_server::SyncService;

/// 全局应用状态（由 Tauri 管理）
pub struct AppState {
    pub db: Arc<Db>,
    pub data_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub log_dir: PathBuf,
    /// 局域网同步服务（默认关闭，由用户主动开启）
    pub sync: SyncService,
    /// 调度器唤醒信号：Mutex<bool> + Condvar
    pub wake: Arc<(Mutex<bool>, Condvar)>,
}

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf, backup_dir: PathBuf, log_dir: PathBuf) -> Self {
        let db = Arc::new(db);
        let sync = SyncService::new(db.clone(), data_dir.join("sync"));
        Self {
            db,
            data_dir,
            backup_dir,
            log_dir,
            sync,
            wake: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// 数据变化后立即唤醒调度器重新计算
    pub fn notify_scheduler(&self) {
        let (lock, cv) = &*self.wake;
        let mut flag = lock.lock().unwrap_or_else(|e| e.into_inner());
        *flag = true;
        cv.notify_all();
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
