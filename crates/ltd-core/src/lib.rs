//! Local To Do 平台无关核心。
//!
//! 这里只包含数据模型、SQLite 访问、重复规则与提醒计算，不依赖 Tauri，
//! 因此可同时被 Windows 桌面端与 Android 端复用。

pub mod db;
pub mod domain;
pub mod error;
pub mod models;
pub mod repo;
pub mod sync;
