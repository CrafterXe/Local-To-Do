# 局域网配对与同步（Windows → Android）

本文档描述 `feature/lan-sync-pairing` 支线实现的功能边界、架构、安全机制与构建方式。

> 一句话定位：**Windows 桌面端是唯一权威数据源，Android 端是只读伴侣。**
> 数据方向固定为 **Windows → Android**，不存在双向同步，也不存在手机端写入。

---

## 1. 功能定位

该支线把手机变成电脑数据的「随身影子」：在家庭局域网内，手机扫码与电脑配对，
之后自动拉取全量快照并持续增量同步，供用户随时离线浏览任务、清单和日历。

支线包含的能力：

| 端 | 能力 |
| --- | --- |
| Windows | SQLite 同步日志（`sync_log`）、全量快照、增量拉取、自签名 HTTPS 同步服务、配对码生成与校验、设备 Token 认证、设备撤销、同步状态接口、证书与指纹生成 |
| Android | 二维码配对、TLS 证书 SHA-256 指纹锁定、Keystore 加密保存 Token、首次全量快照、增量同步、同步重置后重新拉取、独立 SQLite 镜像库、只读查询（我的今天 / 全部任务 / 日历 / 我的清单 / 任务详情）、离线浏览最近一次成功同步的数据 |

支线**不包含**（仍保留在 `feature/android-background-reminders` 分支）：

- Android 后台定时提醒（AlarmManager 调度、开机恢复）；
- 第五阶段的双端使用说明页面。

---

## 2. 整体架构

```
┌──────────────────────────── Windows ─────────────────────────────┐
│  SQLite 主库 (local-to-do.db)                                     │
│      │ 业务写入与 sync_log 记录同一事务提交                          │
│      ▼                                                            │
│  ltd-core::sync  ── snapshot() / changes_since(rev)               │
│      │                                                            │
│  sync_server (Axum + rustls, 独立 Tokio 线程, 仅绑定私有 IPv4)      │
│      └─ /status  /pair  /sync/status  /sync/snapshot  /sync/changes│
└───────────────────────────────┬───────────────────────────────────┘
                                │ HTTPS（自签名证书 + 指纹锁定）
┌───────────────────────────────▼───────────────────────────────────┐
│  Android                                                          │
│  sync_client：pairing → http(pinned) → engine                     │
│      ├─ secrets：Keystore AES/GCM 加密 Token（SharedPreferences） │
│      ├─ mirror：独立 SQLite 镜像库（只由同步结果写入）             │
│      └─ mirror_read：只读查询（今天/全部/日历/清单/详情）           │
└───────────────────────────────────────────────────────────────────┘
```

关键代码位置：

- `crates/ltd-core/src/sync.rs`：同步日志、快照、增量协议（平台无关）。
- `crates/ltd-core/src/mirror.rs` / `mirror_read.rs`：Android 镜像库写入与只读查询。
- `src-tauri/src/sync_server/`：Windows 端 HTTPS 服务（`tls.rs` / `routes.rs` / `store.rs`）。
- `src-tauri/src/sync_client/`：Android 端配对、指纹锁定、同步引擎与安全存储。
- `src/mobile/`：Android 只读界面。

---

## 3. 数据流向

1. 用户在 Windows 上正常增删改任务 → 同一 SQLite 事务内追加 `sync_log` 记录（全局自增 `rev`）。
2. Android 首次同步调用 `/sync/snapshot`，获得与某个 `rev` 一致的**全量快照**（一致性快照，不会读到半途状态）。
3. 之后调用 `/sync/changes?since=<rev>` 按页获取增量（客户端每页 500 条）。
4. `delete` 以墓碑形式下发；`reset` 表示需要丢弃本地游标并重新拉全量。
5. Android 把结果写入**自己的**镜像库，UI 只读该镜像库。

Android 端没有任何写业务数据的命令——镜像库只由同步引擎写入。

---

## 4. 配对流程

1. Windows 打开「同步设置」，启动同步服务，生成有效期为 5 分钟的 8 位数字配对码，
   并渲染二维码。二维码内容：

   ```json
   {"type":"ltd-pair","v":1,"protocol":"1","host":"192.168.x.x","port":38421,
    "fingerprint":"AA:BB:...","code":"12345678","expiresAt":1730000000000}
   ```

2. Android 扫描二维码，先在本地校验：协议版本、`type`、私网 IPv4、端口范围、
   指纹格式、配对码格式与是否过期——**任何一项不通过都不会发起网络请求**。
3. Android 携带配对码调用 `POST /pair`，服务端校验配对码有效性与来源地址后，
   签发设备 Token。
4. Android 将 Token 用 Keystore 密钥加密后写入 SharedPreferences，并在连接配置中
   保存主机、端口、证书指纹（不保存 Token 明文）。
5. 用户在 Windows 的同步设置里可以看到已配对设备，并可**撤销**；撤销后该设备的
   Token 立即失效，Android 侧同步会失败并提示重新配对。

---

## 5. TLS 安全机制

- Windows 首次启动同步服务时生成**本地自签名 CA + 服务器证书**，私钥只存放于应用数据目录，
  任何接口都不会返回私钥，也不写入日志。
- Android 不依赖系统信任链，而是比对**服务器证书的 SHA-256 指纹**（大写、冒号分隔）。
  每次同步都会重新校验，指纹不符立即失败。
- 校验失败时**绝不清空本地镜像**，保证离线数据不丢。
- 证书更换（例如重置配对）意味着必须重新扫码配对。

---

## 6. Token 认证与存储

- Token 由服务端随机生成，仅通过配对接口返回一次。
- Android 使用系统 Keystore 中的 AES/GCM 密钥加密 Token，密文写入 SharedPreferences；
  密钥不可导出，卸载应用即失效。
- Keystore 解密失败（例如系统升级或密钥被清除）时不会崩溃：
  客户端会提示需要重新配对（见提交 `776153a`）。
- 桌面端仅保留一个开发调试用的文件存储实现，Android 构建不会使用它。

---

## 7. 首次快照与增量同步

- 快照与游标基于同一个 `rev` 取得，避免「快照 + 增量」之间出现空洞。
- 增量按 `rev` 升序分页，客户端保存每页进度，中断后可续传。
- 服务端限制单页上限 1000 条，客户端使用 500 条。
- 「同步重置」会清空 Android 镜像并重新拉取全量快照。

---

## 8. 离线查看

镜像库是普通 SQLite 文件，Android 启动时直接读取，不需要网络。

- 有网络时进入同步页可手动/自动同步；
- 无网络时仍可浏览**最近一次成功同步**的数据；
- UI 会显示上次同步时间，便于判断数据新鲜度。

---

## 9. Android 只读边界

- `crates/ltd-core/src/mirror_read.rs` 只提供查询函数，没有写接口。
- Android 端命令列表不包含任何任务/清单的增删改。
- 唯一会写镜像库的是同步引擎，写入内容全部来自 Windows。
- 因此 Android 上的「完成/编辑/删除」等操作在协议层就不存在，不是靠 UI 隐藏。

---

## 10. 已知限制

- 仅支持家庭/私有局域网使用，服务端拒绝绑定公网地址。
- Windows 是唯一权威数据源，Android 只读；当前不是双向同步。
- Android 不能编辑或完成任务。
- 真实摄像头扫码需要独立设备验收（模拟器/调试环境仅验证协议）。
- 该预览版本未完成全部正式发布测试。
- Android 后台定时提醒不属于本支线。

---

## 11. 在新环境初始化与构建

### 11.1 前置条件

- Node.js 18+、Rust stable（含 `aarch64-linux-android` target）、Android SDK + NDK。
- 环境变量：`ANDROID_HOME`、`NDK_HOME`（或 Tauri 自动探测）。

### 11.2 桌面端（Windows）

```powershell
npm ci
npm run tauri build
```

### 11.3 Android 原生工程

`src-tauri/gen/` 是 **Tauri 自动生成的目录，已被 `.gitignore` 忽略**，不需要也不应该提交。
新环境按以下步骤重新生成：

```powershell
npm ci
npm run tauri android init
npm run tauri android build -- --apk --target aarch64
```

生成后可用的产物位于：

```
src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release.apk
```

说明：

- **本支线不需要任何自定义 Android 原生源码。**
  二维码扫码所需的 `CAMERA`、`VIBRATE` 权限与 camera 特性由
  `tauri-plugin-barcode-scanner` 的 Android 库通过 Manifest 合并自动引入；
  `INTERNET` 由 Tauri 模板提供；通知权限由 `tauri-plugin-notification` 提供。
- Android Keystore 加密完全由 Rust 通过 JNI 实现（`sync_client/secrets.rs`），
  不需要额外的 Kotlin 文件。
- 若需要发布签名，请自行在本地 `app/build.gradle.kts` 中配置签名信息；
  **不要**把密钥库或密码提交到仓库。

### 11.4 校验顺序建议

1. `cargo test`（桌面端与核心协议测试）。
2. 桌面端启动同步服务，确认能看到二维码与设备列表。
3. Android 安装后扫码配对，确认首次快照与增量同步成功。
4. 断开网络，确认可离线浏览上次同步的数据。
5. 在 Windows 撤销设备，确认 Android 同步被拒绝并提示重新配对。
