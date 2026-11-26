//! 设备令牌的安全存储。
//!
//! Android 使用系统 Keystore 中的 AES/GCM 密钥加密令牌，密文写入
//! SharedPreferences；密钥不可导出，卸载应用即失效。
//! 桌面端仅为开发调试提供文件存储，Android 构建不会使用它。
//! 令牌不写入日志、不写入连接配置文件。

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub trait SecretStore: Send + Sync {
    fn load_token(&self) -> Result<Option<String>, String>;
    fn save_token(&self, token: &str) -> Result<(), String>;
    fn delete_token(&self) -> Result<(), String>;
}

pub fn default_store(dir: &Path) -> Arc<dyn SecretStore> {
    #[cfg(target_os = "android")]
    {
        let _ = dir;
        Arc::new(android::AndroidKeystoreStore)
    }
    #[cfg(not(target_os = "android"))]
    {
        Arc::new(FileSecretStore::new(dir.join("dev-token")))
    }
}

/// 桌面开发用：令牌以独立文件保存（非 Android 目标，不参与产品配对流程）
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SecretStore for FileSecretStore {
    fn load_token(&self) -> Result<Option<String>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let text = text.trim().to_string();
                Ok(if text.is_empty() { None } else { Some(text) })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("读取本地令牌失败：{e}")),
        }
    }

    fn save_token(&self, token: &str) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建令牌目录失败：{e}"))?;
        }
        std::fs::write(&self.path, token).map_err(|e| format!("保存本地令牌失败：{e}"))
    }

    fn delete_token(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删除本地令牌失败：{e}")),
        }
    }
}

#[cfg(test)]
pub struct MemorySecretStore {
    value: std::sync::Mutex<Option<String>>,
}

#[cfg(test)]
impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            value: std::sync::Mutex::new(None),
        }
    }

    pub fn peek(&self) -> Option<String> {
        self.value.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn load_token(&self) -> Result<Option<String>, String> {
        Ok(self.peek())
    }

    fn save_token(&self, token: &str) -> Result<(), String> {
        *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.to_string());
        Ok(())
    }

    fn delete_token(&self) -> Result<(), String> {
        *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::SecretStore;

    const PREFS_NAME: &str = "ltd_secure";
    const TOKEN_KEY: &str = "sync_token";
    const KEY_ALIAS: &str = "ltd_sync_token_key";

    pub struct AndroidKeystoreStore;

    impl SecretStore for AndroidKeystoreStore {
        fn load_token(&self) -> Result<Option<String>, String> {
            jni_store::load()
        }

        fn save_token(&self, token: &str) -> Result<(), String> {
            jni_store::save(token)
        }

        fn delete_token(&self) -> Result<(), String> {
            jni_store::delete()
        }
    }

    /// JNI 调用 Android Keystore + SharedPreferences。
    mod jni_store {
        use jni::objects::{JByteArray, JObject, JString, JValue};
        use jni::sys::jobject;
        use jni::{JNIEnv, JavaVM};

        use super::{KEY_ALIAS, PREFS_NAME, TOKEN_KEY};

        const PURPOSE_ENCRYPT_DECRYPT: i32 = 3;

        fn js<E: std::fmt::Display>(e: E) -> String {
            format!("Android 安全存储错误：{e}")
        }

        fn with_env<T>(
            f: impl for<'a> FnOnce(&mut JNIEnv<'a>, &JObject<'a>) -> Result<T, String>,
        ) -> Result<T, String> {
            let ctx = ndk_context::android_context();
            let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
                .map_err(|e| format!("无法获取 Android JavaVM：{e}"))?;
            let mut guard = vm
                .attach_current_thread()
                .map_err(|e| format!("无法附加 Android 线程：{e}"))?;
            let env: &mut JNIEnv = &mut guard;
            env.push_local_frame(16).map_err(js)?;
            let context = unsafe { JObject::from_raw(ctx.context() as jobject) };
            let result = f(env, &context);
            // 挂起的 Java 异常必须在这里清除：JNI 线程带着未捕获异常退出会直接
            // 终止进程（例如 GCM IV 长度非法、密文校验失败时）。
            let pending = env.exception_check().unwrap_or(false);
            if pending {
                let _ = env.exception_clear();
            }
            unsafe {
                let _ = env.pop_local_frame(&JObject::null());
            }
            match (result, pending) {
                (Ok(value), false) => Ok(value),
                (Err(message), false) => Err(message),
                _ => Err("本地安全存储异常，请重新扫码配对".to_string()),
            }
        }

        fn preferences<'a>(
            env: &mut JNIEnv<'a>,
            context: &JObject<'a>,
        ) -> Result<JObject<'a>, String> {
            let name = env.new_string(PREFS_NAME).map_err(js)?;
            env.call_method(
                context,
                "getSharedPreferences",
                "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
                &[JValue::Object(&name), JValue::Int(0)],
            )
            .and_then(|v| v.l())
            .map_err(js)
        }

        fn load_stored() -> Result<Option<String>, String> {
            with_env(|env, context| {
                let prefs = preferences(env, context)?;
                let key = env.new_string(TOKEN_KEY).map_err(js)?;
                let stored = env
                    .call_method(
                        &prefs,
                        "getString",
                        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                        &[JValue::Object(&key), JValue::Object(&JObject::null())],
                    )
                    .and_then(|v| v.l())
                    .map_err(js)?;
                if stored.is_null() {
                    return Ok(None);
                }
                let stored = unsafe { JString::from_raw(stored.into_raw()) };
                let text: String = env.get_string(&stored).map_err(js)?.into();
                if text.is_empty() {
                    return Ok(None);
                }
                Ok(Some(text))
            })
        }

        fn secret_key<'a>(env: &mut JNIEnv<'a>) -> Result<JObject<'a>, String> {
            let algorithm = env.new_string("AndroidKeyStore").map_err(js)?;
            let keystore = env
                .call_static_method(
                    "java/security/KeyStore",
                    "getInstance",
                    "(Ljava/lang/String;)Ljava/security/KeyStore;",
                    &[JValue::Object(&algorithm)],
                )
                .and_then(|v| v.l())
                .map_err(js)?;
            env.call_method(
                &keystore,
                "load",
                "(Ljava/security/KeyStore$LoadStoreParameter;)V",
                &[JValue::Object(&JObject::null())],
            )
            .map_err(js)?;

            let alias = env.new_string(KEY_ALIAS).map_err(js)?;
            let exists = env
                .call_method(
                    &keystore,
                    "containsAlias",
                    "(Ljava/lang/String;)Z",
                    &[JValue::Object(&alias)],
                )
                .and_then(|v| v.z())
                .map_err(js)?;
            if !exists {
                generate_key(env, &alias)?;
            }
            env.call_method(
                &keystore,
                "getKey",
                "(Ljava/lang/String;[C)Ljava/security/Key;",
                &[JValue::Object(&alias), JValue::Object(&JObject::null())],
            )
            .and_then(|v| v.l())
            .map_err(js)
        }

        fn generate_key<'a>(env: &mut JNIEnv<'a>, alias: &JString<'a>) -> Result<(), String> {
            let aes = env.new_string("AES").map_err(js)?;
            let provider = env.new_string("AndroidKeyStore").map_err(js)?;
            let generator = env
                .call_static_method(
                    "javax/crypto/KeyGenerator",
                    "getInstance",
                    "(Ljava/lang/String;Ljava/lang/String;)Ljavax/crypto/KeyGenerator;",
                    &[JValue::Object(&aes), JValue::Object(&provider)],
                )
                .and_then(|v| v.l())
                .map_err(js)?;

            let builder = env
                .new_object(
                    "android/security/keystore/KeyGenParameterSpec$Builder",
                    "(Ljava/lang/String;I)V",
                    &[JValue::Object(alias), JValue::Int(PURPOSE_ENCRYPT_DECRYPT)],
                )
                .map_err(js)?;
            let mode = env.new_string("GCM").map_err(js)?;
            let modes = env
                .new_object_array(1, "java/lang/String", &mode)
                .map_err(js)?;
            let padding = env.new_string("NoPadding").map_err(js)?;
            let paddings = env
                .new_object_array(1, "java/lang/String", &padding)
                .map_err(js)?;
            env.call_method(
                &builder,
                "setBlockModes",
                "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
                &[JValue::Object(&modes)],
            )
            .map_err(js)?;
            env.call_method(
                &builder,
                "setEncryptionPaddings",
                "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
                &[JValue::Object(&paddings)],
            )
            .map_err(js)?;
            let spec = env
                .call_method(
                    &builder,
                    "build",
                    "()Landroid/security/keystore/KeyGenParameterSpec;",
                    &[],
                )
                .and_then(|v| v.l())
                .map_err(js)?;
            env.call_method(
                &generator,
                "init",
                "(Ljava/security/spec/AlgorithmParameterSpec;)V",
                &[JValue::Object(&spec)],
            )
            .map_err(js)?;
            env.call_method(&generator, "generateKey", "()Ljavax/crypto/SecretKey;", &[])
                .map_err(js)?;
            Ok(())
        }

        fn cipher<'a>(env: &mut JNIEnv<'a>) -> Result<JObject<'a>, String> {
            let transform = env.new_string("AES/GCM/NoPadding").map_err(js)?;
            env.call_static_method(
                "javax/crypto/Cipher",
                "getInstance",
                "(Ljava/lang/String;)Ljavax/crypto/Cipher;",
                &[JValue::Object(&transform)],
            )
            .and_then(|v| v.l())
            .map_err(js)
        }

        fn byte_array<'a>(obj: JObject<'a>) -> JByteArray<'a> {
            unsafe { JByteArray::from_raw(obj.into_raw()) }
        }

        fn encrypt(key: &JObject<'_>, token: &str) -> Result<String, String> {
            with_env(|env, _context| {
                let cipher = cipher(env)?;
                env.call_method(
                    &cipher,
                    "init",
                    "(ILjava/security/Key;)V",
                    &[JValue::Int(1), JValue::Object(key)],
                )
                .map_err(js)?;
                let plain = env.byte_array_from_slice(token.as_bytes()).map_err(js)?;
                let encrypted = env
                    .call_method(&cipher, "doFinal", "([B)[B", &[JValue::Object(&plain)])
                    .and_then(|v| v.l())
                    .map_err(js)?;
                let iv = env
                    .call_method(&cipher, "getIV", "()[B", &[])
                    .and_then(|v| v.l())
                    .map_err(js)?;
                let encrypted = env.convert_byte_array(&byte_array(encrypted)).map_err(js)?;
                let iv = env.convert_byte_array(&byte_array(iv)).map_err(js)?;
                Ok(format!("{}:{}", hex::encode(iv), hex::encode(encrypted)))
            })
        }

        fn decrypt(key: &JObject<'_>, iv: &[u8], data: &[u8]) -> Result<String, String> {
            with_env(|env, _context| {
                let cipher = cipher(env)?;
                let iv = env.byte_array_from_slice(iv).map_err(js)?;
                let spec = env
                    .new_object(
                        "javax/crypto/spec/GCMParameterSpec",
                        "(I[B)V",
                        &[JValue::Int(128), JValue::Object(&iv)],
                    )
                    .map_err(js)?;
                env.call_method(
                    &cipher,
                    "init",
                    "(ILjava/security/Key;Ljava/security/spec/AlgorithmParameterSpec;)V",
                    &[JValue::Int(2), JValue::Object(key), JValue::Object(&spec)],
                )
                .map_err(js)?;
                let data = env.byte_array_from_slice(data).map_err(js)?;
                let plain = env
                    .call_method(&cipher, "doFinal", "([B)[B", &[JValue::Object(&data)])
                    .and_then(|v| v.l())
                    .map_err(js)?;
                let bytes = env.convert_byte_array(&byte_array(plain)).map_err(js)?;
                String::from_utf8(bytes).map_err(|_| "本地令牌解码失败".to_string())
            })
        }

        pub fn load() -> Result<Option<String>, String> {
            let Some(stored) = load_stored()? else {
                return Ok(None);
            };
            let (iv_hex, data_hex) = stored
                .split_once(':')
                .ok_or_else(|| "本地令牌数据格式无效".to_string())?;
            let iv = hex::decode(iv_hex).map_err(|_| "本地令牌数据损坏".to_string())?;
            let data = hex::decode(data_hex).map_err(|_| "本地令牌数据损坏".to_string())?;
            // Android Keystore 的 GCM 只接受 12 字节 IV；长度不符时必须在调用
            // JNI 之前拒绝，否则会抛出未捕获异常导致崩溃。
            if iv.len() != 12 || data.len() < 16 {
                return Err("本地令牌数据损坏，请重新扫码配对".to_string());
            }
            with_env(|env, _context| {
                let key = secret_key(env)?;
                decrypt(&key, &iv, &data).map(Some)
            })
            .map_err(|_| "本地令牌无法解密，请重新扫码配对".to_string())
        }

        pub fn save(token: &str) -> Result<(), String> {
            let encrypted = with_env(|env, _context| {
                let key = secret_key(env)?;
                encrypt(&key, token)
            })?;
            with_env(|env, context| {
                let prefs = preferences(env, context)?;
                let editor = env
                    .call_method(
                        &prefs,
                        "edit",
                        "()Landroid/content/SharedPreferences$Editor;",
                        &[],
                    )
                    .and_then(|v| v.l())
                    .map_err(js)?;
                let key = env.new_string(TOKEN_KEY).map_err(js)?;
                let value = env.new_string(&encrypted).map_err(js)?;
                env.call_method(
                    &editor,
                    "putString",
                    "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
                    &[JValue::Object(&key), JValue::Object(&value)],
                )
                .map_err(js)?;
                let saved = env
                    .call_method(&editor, "commit", "()Z", &[])
                    .and_then(|v| v.z())
                    .map_err(js)?;
                if !saved {
                    return Err("保存加密令牌失败".into());
                }
                Ok(())
            })
        }

        pub fn delete() -> Result<(), String> {
            with_env(|env, context| {
                let prefs = preferences(env, context)?;
                let editor = env
                    .call_method(
                        &prefs,
                        "edit",
                        "()Landroid/content/SharedPreferences$Editor;",
                        &[],
                    )
                    .and_then(|v| v.l())
                    .map_err(js)?;
                let key = env.new_string(TOKEN_KEY).map_err(js)?;
                env.call_method(
                    &editor,
                    "remove",
                    "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
                    &[JValue::Object(&key)],
                )
                .map_err(js)?;
                let removed = env
                    .call_method(&editor, "commit", "()Z", &[])
                    .and_then(|v| v.z())
                    .map_err(js)?;
                if !removed {
                    return Err("删除本地令牌失败".into());
                }
                Ok(())
            })
        }
    }
}
