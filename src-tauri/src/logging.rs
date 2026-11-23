use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(log_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(log_dir)?;
    let path = log_dir.join("app.log");
    // 超过 2MB 时轮转
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 2 * 1024 * 1024 {
            let rotated = log_dir.join("app.old.log");
            let _ = fs::remove_file(&rotated);
            let _ = fs::rename(&path, rotated);
        }
    }
    let mut guard = LOG_PATH.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(path);
    Ok(())
}

pub fn log(message: impl AsRef<str>) {
    let line = format!(
        "[{}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        message.as_ref()
    );
    eprint!("{line}");
    let path = {
        let guard = LOG_PATH.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    };
    if let Some(p) = path {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

pub fn info(message: impl AsRef<str>) {
    log(format!("INFO  {}", message.as_ref()));
}

pub fn warn(message: impl AsRef<str>) {
    log(format!("WARN  {}", message.as_ref()));
}

pub fn error(message: impl AsRef<str>) {
    log(format!("ERROR {}", message.as_ref()));
}
