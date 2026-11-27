# Local To Do Android 独立离线版

## 1. 交付物与平台边界

- 版本：1.2.1（`versionCode 1002001`）
- 包名：`com.localtodo.mobile`
- 最低系统：Android 7.0（API 24）；目标 API 37
- APK 架构：`arm64-v8a`、`x86_64`
- 构建类型：Debug，使用调试签名，不是正式发布签名包
- APK 路径：`src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`
- 文件大小：602,330,217 字节（约 574.5 MiB；未优化调试原生库是体积较大的主要原因）
- SHA256：`16EF2795D6527AEF41DBC315D5861BE53B7B67645077AB400CD46FCE23EC2DB0`

Android 版是独立应用，不是 Windows 伴侣程序。任务、步骤、清单、备忘录、重复规则、设置和历史记录保存在应用私有目录的 `local-to-do.db`；该 SQLite 数据库是唯一权威数据源。无需账号、网络、Windows 电脑或旧版同步服务，也不以 `mirror.db` 为主库。

APK 未声明 `INTERNET` 或 `CAMERA` 权限，`usesCleartextTraffic=false`。包名与旧 `com.localtodo.desktop` 不同，可作为独立应用并存。

## 2. 安装与数据安全

### 使用 ADB 安装

```powershell
# 首次安装
adb install app-universal-debug.apk

# 覆盖升级，保留应用数据
adb install -r app-universal-debug.apk
```

首次启动会创建私有数据库和默认清单“任务”。Android 13 及以上需要允许通知权限。设置页可继续打开精确闹钟和电池优化设置。

数据位置：

```text
/data/user/0/com.localtodo.mobile/
├── local-to-do.db
├── local-to-do.db-wal
├── local-to-do.db-shm
├── backups/   # JSON、SQLite 快照及导出的 CSV
└── logs/
```

**卸载应用会删除这些私有数据。卸载或恢复备份前，请先在“设置 → 备份、导出与恢复”中创建并分享 JSON 备份。**

## 3. 使用说明

### 导航与周数

- 左上角展开完整导航，首项为“全部任务”，其后为“我的一天”、日历、清单、重要任务、已计划等。
- 抽屉主 Logo 右侧显示当前周数，与电脑版共用 `WeekPicker` 和周数规则。
- 点击周数可浏览月份并选择第一周基准日期；设置页也可修改日期或恢复默认周数。
- 启动器与抽屉均使用电脑版蓝紫渐变勾选图标。

### 任务、清单和备忘录

- 在任务视图顶部输入标题并点击“添加”；点击任务打开覆盖式详情。
- 详情支持步骤、到期日期与时间、提醒、重复规则、所属清单、重要、“我的一天”和备注；修改自动保存。
- “我的清单”支持新建、改色、上下移动排序、删除；连同任务删除后可在“最近删除”整体恢复。
- 备忘录支持新建、搜索、标题与正文编辑，约 0.5 秒自动保存。
- 日历显示有任务的日期；重要、我的一天、已计划、已完成和历史记录使用同一权威数据库。
- 系统返回键优先关闭当前抽屉或详情，不直接丢失列表状态。

### Android 原生提醒

提醒由 Kotlin `AlarmManager` 和 `NotificationManager` 调度，不依赖 WebView 页面定时器。应用记录已调度、已触发和已展示状态，并处理开机、覆盖升级、日期/时间/时区广播。

未授予精确闹钟时仍使用系统非精确闹钟。厂商后台策略、Doze、省电模式和电池白名单都可能影响实际到达时间，不能保证所有设备绝对准时。

### 备份、导入和分享

“设置 → 备份、导出与恢复”支持：

- 立即生成本机 JSON + SQLite 快照；
- 备份并通过 Android 系统分享 JSON；
- 导出并通过系统分享 CSV；
- 从本机备份列表恢复；
- 通过文件入口导入 JSON，校验报告后二次确认并覆盖恢复。

恢复在同一事务中替换业务数据并重建提醒；校验或写入失败时回滚，不保留部分数据。

## 4. 从源码构建

### 环境

- Windows PowerShell
- Node.js 24.21.0、npm
- Rust 1.100.0，以及 `aarch64-linux-android`、`x86_64-linux-android` target
- OpenJDK 17
- Android SDK、Platform Tools、NDK 27.2.12479018
- Tauri CLI 2.12.6（由项目 npm 依赖提供）

```powershell
npm ci
rustup target add aarch64-linux-android x86_64-linux-android
```

标准 Android 工程已保留在 `src-tauri/gen/android`；仅在该目录缺失时才需要重新执行 `npm run tauri android init -- --ci --skip-targets-install`。

### 常规构建

允许创建符号链接的 Windows 环境可直接执行：

```powershell
npm run tauri android build -- --debug --ci --target aarch64 x86_64 --apk
```

### 当前机器未开启开发者模式时

Tauri CLI 会在 Rust 编译成功后尝试创建 `.so` 符号链接，并报 `Creation symbolic link is not allowed for this system.`。不修改系统权限时，按以下方式复制原生库，再让 Gradle 跳过重复 Rust 构建。

分别执行两种架构的 CLI 构建：

```powershell
npm run tauri android build -- --debug --ci --target aarch64 --apk
Copy-Item -Force "src-tauri\target\aarch64-linux-android\debug\liblocal_to_do_lib.so" `
  "src-tauri\gen\android\app\src\main\jniLibs\arm64-v8a\liblocal_to_do_lib.so"

npm run tauri android build -- --debug --ci --target x86_64 --apk
Copy-Item -Force "src-tauri\target\x86_64-linux-android\debug\liblocal_to_do_lib.so" `
  "src-tauri\gen\android\app\src\main\jniLibs\x86_64\liblocal_to_do_lib.so"

Copy-Item -Recurse -Force "dist\*" "src-tauri\gen\android\app\src\main\assets\"
Set-Location "src-tauri\gen\android"
.\gradlew.bat :app:assembleUniversalDebug -PskipRustBuild
```

**只有日志明确显示 Rust `Finished dev`，且后续唯一阻断是符号链接权限时，才可继续复制和 Gradle 打包。Rust 或前端编译错误不能被忽略。** 前端修改后必须重新执行 CLI 原生库编译；仅替换 assets 或重复 Gradle 打包可能继续使用原生库内嵌的旧前端。

## 5. 真实测试报告（2026-11-27）

### 自动化与构建

| 项目 | 实际结果 |
|---|---|
| `cargo test --workspace` | 167 项通过，0 失败：Tauri 115、Core 35、数据库黄金测试 7、提醒状态 3、任务行为 3、黄金向量 4 |
| `npm test` | 7 个测试文件、58 项通过，0 失败 |
| `npm run build` | TypeScript 检查与 Vite 构建通过 |
| `gradlew.bat :app:assembleUniversalDebug -PskipRustBuild` | 通过，生成双架构通用 Debug APK |
| `gradlew.bat :app:testUniversalDebugUnitTest -PskipRustBuild` | 编译检查通过；该任务为 `NO-SOURCE`，不冒充原生单元测试数量 |
| `aapt dump permissions/badging` | 包名与 ABI 正确；无 `INTERNET`、`CAMERA` 权限 |
| `git diff --check` | 无空白错误 |

### API 35 模拟器实测

| 场景 | 实际证据 |
|---|---|
| 最新三项界面反馈 | 系统应用信息页显示蓝紫勾选图标；抽屉首项为“全部任务”；周数位于 Logo 品牌行右侧 |
| 周数选择与持久化 | 选择 2026-11-01 后从“第48周”变为“第5周”；冷启动后仍保持；月历弹层未越出屏幕 |
| 离线能力 | 关闭 Wi-Fi/移动数据后可创建任务；APK 无网络权限 |
| 任务与清单 | 创建 `E2E_验收`，清单顺序由“任务、E2E_验收”改为反向；任务包含步骤、备注、到期时间和提醒 |
| 清单删除恢复 | 删除清单及 2 条任务后出现在“最近删除”；整体恢复后 2 条任务均存在 |
| 备忘录 | 标题与正文自动保存，关闭后重开内容一致 |
| 日历与视图 | 当日历出现任务标记；重要、我的一天、已计划、已完成和历史记录均显示对应任务/操作 |
| 主题 | 浅色与深色切换正常；浅色跨进程冷启动和设备重启保持 |
| 实际提醒 | 13:20 的系统通知显示任务标题、到期时间和清单；通知栏截图已核验 |
| 进程回收 | 后台执行 `am kill` 后系统闹钟仍在，实际通知到达；冷启动后应用内显示“提醒已触发” |
| 覆盖升级 | `adb install -r` 后，13:32 的未来提醒仍以原 UTC 时间注册 |
| 开机恢复 | 模拟器重启后、未操作任务前，13:32 的提醒已由开机接收器恢复注册 |
| 取消提醒 | 关闭任务提醒后原生 `scheduled` 从 1 变为 0；系统闹钟日志出现 `alarm_cancelled` |
| 本机备份 | 生成 JSON（6,544 字节）和 SQLite 快照（118,784 字节）；统计为 2 任务、2 清单、1 备忘录 |
| JSON/CSV 分享 | Android 分享面板分别显示正确 `.json` 和 `.csv` 文件；未实际发送到第三方应用 |
| CSV 内容 | 表头及两条测试任务、备注、日期、时间和提醒偏移正确 |
| 本机覆盖恢复 | 恢复前临时任务被移除，备份中的 2 任务、2 清单、1 备忘录恢复 |
| 文件入口导入 | 通过隐藏文件输入选择同一 JSON，出现校验报告和二次确认；恢复结果与本机恢复一致 |

### Android 10 真机实测

设备：`CDY-AN20`。使用 `adb install -r` 保留数据覆盖安装最新 APK，启动正常；原有 1 条任务和 1 个清单仍在。抽屉首项为“全部任务”，Logo 右侧显示原有自定义周数“第3周”，加载的前端哈希与最新 APK 一致。此前已确认真机可独立创建数据库和任务。

真机未执行详细坐标自动化，避免触碰其他应用；完整提醒、重启和备份破坏性流程在隔离模拟器中完成。

## 6. 已知限制与未标记通过的项目

1. 单次最多注册 200 条未来提醒；更晚提醒由设置页显示溢出数量，并在后续校准时滚动补充。
2. 日期/时间/时区广播会恢复已保存的 UTC 调度；自动时区变化后的墙上时间重算仍需下次应用启动或重新校准。
3. 模拟器未授予精确闹钟和电池优化白名单；实际通知已到达，但不据此承诺所有系统与厂商设备绝对准时。
4. Android 10 真机未重复完整提醒触发、开机恢复和覆盖恢复流程；这些项目已在 API 35 模拟器实测。
5. 系统分享仅验证分享面板和文件内容，未登录或操作 Gmail、Drive 等第三方应用。
6. 当前只交付 Debug APK，未生成正式发布签名 APK/AAB，也未执行应用商店发布检查。
