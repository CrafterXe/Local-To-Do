use std::path::Path;

/// 校验提示音文件：必须存在且为 WAV
pub fn validate(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.is_file() {
        return Err("提示音文件不存在".into());
    }
    let is_wav = p
        .extension()
        .map(|e| e.to_string_lossy().eq_ignore_ascii_case("wav"))
        .unwrap_or(false);
    if !is_wav {
        return Err("仅支持 WAV 格式音频文件".into());
    }
    Ok(())
}

/// 播放提示音。`path` 为 None 时播放系统默认通知音。
pub fn play(path: Option<&str>) -> Result<(), String> {
    #[cfg(windows)]
    {
        play_windows(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("当前平台不支持提示音".into())
    }
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn play_windows(path: Option<&str>) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{
        PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_FLAGS, SND_NODEFAULT,
    };

    unsafe {
        match path {
            Some(p) => {
                validate(p)?;
                let wide = to_wide(p);
                let flags = SND_FLAGS(SND_FILENAME.0 | SND_ASYNC.0 | SND_NODEFAULT.0);
                if PlaySoundW(PCWSTR(wide.as_ptr()), None, flags).as_bool() {
                    Ok(())
                } else {
                    Err("无法播放该音频文件".into())
                }
            }
            None => {
                let alias = to_wide("SystemNotification");
                let flags = SND_FLAGS(SND_ALIAS.0 | SND_ASYNC.0 | SND_NODEFAULT.0);
                if PlaySoundW(PCWSTR(alias.as_ptr()), None, flags).as_bool() {
                    Ok(())
                } else if PlaySoundW(PCWSTR::null(), None, SND_FLAGS(SND_ASYNC.0)).as_bool() {
                    Ok(())
                } else {
                    Err("系统默认提示音不可用".into())
                }
            }
        }
    }
}
