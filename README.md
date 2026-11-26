# Local To Do

> 本地优先、完全离线的个人任务与备忘录管理软件（Windows 桌面应用）

**简体中文** · [English](#english)

不依赖云服务、不强制注册账号、不收集任何个人数据，所有数据存储在本机 SQLite 中。

---

## ✨ 特性

### 任务管理

- 快速添加任务：名称、所属清单、到期日期/时间、提醒、重要标记、备注
- 单击圆形按钮完成任务，可恢复未完成状态
- 独立「重要」「计划内」「全部任务」「已完成」「最近删除」「历史记录」视图
- 自定义清单：创建、重命名、删除、颜色标记、任务计数、跨清单移动
- 任务备注与独立笔记分别管理，互不强制转换

### 提醒与重复

- 分钟级提醒，由 Rust 后台调度线程驱动（不依赖前端 `setTimeout`）
- Windows 原生通知，关闭窗口最小化到托盘后提醒仍然有效
- 重复规则：每天、工作日（周一至周五）、每周、每月、每年，以及自定义（每隔 N 天/周/月、指定星期几、指定月日）
- 妥善处理 2 月 29/30/31 日与闰年：目标日超过当月天数时自动收缩到月末
- 完成重复任务自动生成下一周期，支持跳过本次、删除当前/整个系列
- 睡眠唤醒后补发错过的提醒（≤24 小时），去重防止重复触发

### 笔记

- 独立笔记 CRUD、搜索、自动保存，记录创建/修改时间
- 与任务备注数据完全独立

### 数据与安全

- SQLite 本地存储，WAL 模式，自动保存
- 手动 + 自动定时备份，JSON / CSV 导出，恢复前二次确认
- 备份目录最多保留 20 份，另含数据库快照
- 完全断网可用：创建/编辑/删除/提醒/搜索全部本地完成

### Windows 集成

- 开始菜单启动、开机自启开关（默认关闭，需用户主动开启）
- 最小化到系统托盘、从托盘恢复、托盘退出
- 窗口大小与位置记忆
- 深色 / 浅色主题，跟随系统
- 标准卸载流程（NSIS 安装包）

---

## 🛠 技术栈

| 层         | 技术                                  |
| --------- | ----------------------------------- |
| 桌面框架      | Tauri 2                             |
| 前端        | React 19 + TypeScript + Vite        |
| 后端 / 系统集成 | Rust                                |
| 数据存储      | SQLite（rusqlite，bundled）            |
| 样式        | 纯 CSS（CSS 变量主题）                     |
| 测试        | Rust 单元测试 + Vitest                  |
| 安装包       | Tauri NSIS（主）+ Windows IExpress（备用） |

架构上业务逻辑、数据模型、提醒调度与 UI 解耦：`src-tauri/src/domain`（领域规则，平台无关）、`src-tauri/src/repo`（数据访问）、`src-tauri/src/scheduler.rs`（调度）、`src`（React UI）。为未来 Android 端预留了平台无关的数据模型与 JSON 备份格式。

---

## 📁 项目结构

```
.
├── src/                     # React 前端
│   ├── components/          # 三栏界面与各视图组件
│   ├── utils/               # 日期/重复/提醒计算（含单元测试）
│   ├── api.ts               # Tauri invoke 封装
│   └── App.tsx
├── src-tauri/
│   ├── src/
│   │   ├── domain/          # 平台无关领域逻辑（重复规则、提醒计算）
│   │   ├── repo/            # SQLite 数据访问层
│   │   ├── db.rs            # 数据库 Schema 与迁移
│   │   ├── scheduler.rs     # 后台提醒调度线程
│   │   ├── commands.rs      # 暴露给前端的命令
│   │   ├── backup.rs        # 备份 / 导出 / 恢复
│   │   └── tests.rs         # 集成测试
│   ├── tauri.conf.json      # Tauri 配置
│   └── Cargo.toml
├── installer/               # 备用 IExpress 安装脚本（NSIS 不可用时使用）
├── docs/                    # 安装、使用、备份恢复、数据库结构、测试报告、已知问题
└── package.json
```

---

## 🚀 本地构建

### 前置依赖

- [Node.js](https://nodejs.org/) ≥ 20
- [Rust](https://www.rust-lang.org/tools/install)（stable，需 MSVC 工具链）
- Windows 10/11（WebView2 运行时，Win11 自带）

### 安装依赖

```bash
npm install
```

### 开发模式

```bash
npm run tauri dev
```

### 前端测试

```bash
npm test
```

### Rust 测试

```bash
cd src-tauri
cargo test
```

### 打包发布版（生成 NSIS 安装包）

```bash
npm run tauri build
```

产物位于 `src-tauri/target/release/bundle/nsis/`。

> **离线打包提示**：`tauri build` 默认会从 GitHub 下载 NSIS 3.13 与 `nsis_tauri_utils.dll`。如网络受限，可预先将它们放入 `%LOCALAPPDATA%\tauri\NSIS` 缓存（`makensis.exe` 在其 `Bin` 目录，`nsis_tauri_utils.dll` 在 `Plugins\x86-unicode\additional\`），Tauri 会校验哈希后跳过下载。

---

## 📄 文档

- [安装说明](docs/安装说明.md)
- [使用说明](docs/使用说明.md)
- [备份与恢复说明](docs/备份与恢复说明.md)
- [数据库结构说明](docs/数据库结构说明.md)
- [测试报告](docs/测试报告.md)
- [已知问题](docs/已知问题.md)

---

## 🔒 数据与隐私

- 数据目录：`%APPDATA%\com.localtodo.desktop\`（数据库、备份、日志）
- 应用不发起任何网络请求，不收集遥测，不上传数据
- 卸载默认保留个人数据，可在卸载向导中勾选删除

---

## 🗺 路线图

- [ ] Android 客户端（复用领域层与 JSON 备份格式）
- [ ] 拖拽排序清单与任务
- [ ] 富文本 / Markdown 笔记
- [ ] 代码签名安装包

---

## 📄 许可证

待添加（建议 MIT 或 Apache-2.0）。发布前请在本仓库根目录放入 `LICENSE` 文件。

---

## English

**Local To Do** is a local-first, fully offline personal task & note manager for Windows, inspired by Microsoft To Do's interaction model but with full data sovereignty — no cloud, no account, no telemetry. Everything is stored in a local SQLite database.

### Build

```bash
npm install
npm run tauri dev     # development
npm run tauri build   # produce the NSIS installer
```

See [docs/](docs/) for installation, usage, backup, and database details (in Chinese).
