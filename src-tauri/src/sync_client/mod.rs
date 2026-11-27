//! Android 只读同步客户端。
//!
//! 组成：
//! - `pinned`：证书指纹锁定的 TLS 配置；
//! - `http`：基于 rustls 的最小 HTTPS 客户端；
//! - `pairing`：二维码解析与校验；
//! - `secrets`：令牌安全存储（Android Keystore / 开发文件）；
//! - `engine`：配对、快照、增量、reset、状态机与只读查询；
//! - `settings`：Android 本地设置（不参与同步）；
//! - `reminders`：后台提醒计划生成与原生闹钟桥接；
//! - `commands`：Tauri 移动端命令（仅移动端注册）。

// 这些代码在所有平台编译（便于桌面测试），命令仅在移动端注册
#![allow(dead_code)]

pub(crate) mod commands;
mod engine;
mod http;
mod pairing;
mod pinned;
pub(crate) mod reminders;
mod secrets;
mod settings;
#[cfg(test)]
mod tests;

pub use engine::MobileSyncService;
pub use secrets::default_store;

/// 与 Windows 服务端一致的同步协议版本
pub const PROTOCOL_VERSION: &str = "1";
