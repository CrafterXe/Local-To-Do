use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Db(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Csv(csv::Error),
    Other(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Db(e) => write!(f, "数据库错误: {e}"),
            AppError::Io(e) => write!(f, "文件错误: {e}"),
            AppError::Json(e) => write!(f, "JSON 错误: {e}"),
            AppError::Csv(e) => write!(f, "CSV 错误: {e}"),
            AppError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e)
    }
}

impl From<csv::Error> for AppError {
    fn from(e: csv::Error) -> Self {
        AppError::Csv(e)
    }
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// 生成错误字符串
pub fn err<T>(msg: impl Into<String>) -> AppResult<T> {
    Err(AppError::Other(msg.into()))
}
