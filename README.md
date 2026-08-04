# DiskLens

> Windows 磁盘占用分析器 —— 极速扫描你的磁盘，看清空间都去哪儿了。

![License](https://img.shields.io/badge/license-MIT-green)
![Platform](https://img.shields.io/badge/platform-Windows-0078d6)
![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24c8db)

DiskLens 是一个基于 **Tauri 2 + React 18 + Rust** 的 Windows 磁盘空间分析工具。它用多线程并行遍历（Rayon）在几秒钟内扫完整块磁盘，以**可折叠的树形列表**呈现每个目录/文件占用，帮你在堆积如山的文件中快速定位大文件、大目录。

项目初期对标 [TreeSize Free](https://www.jam-software.com/treesize_free)，但目标是更轻量、更快速、无商业限制的 MIT 开源替代品。

---

## ✨ 功能特性

- **极速扫描**：Rust 多线程并行遍历（Rayon 本地线程池），C: 盘（约 274 GB / 120 万文件）实测 **约 8 秒**扫完。
- **树形列表**：可展开/折叠的目录树，目录显示 `大小 名称`，散文件折叠进 `N [files]` 虚拟节点，展开即可查看。
- **真实占用空间**：`占用分配空间` 列按卷簇大小向上取整（小文件至少占一个 4K 簇），与 Windows 资源管理器的“占用空间”一致。
- **排序**：默认按大小降序，点击任意表头可切换升/降序。
- **丰富的列信息**：名称 / 占用分配空间 / 文件数 / 目录数 / 占父目录 %（带背景进度条）/ 最后修改日期。
- **扫描目标选择器**：下拉菜单列出本机真实磁盘（含类型与卷标）、最近 5 次扫描路径、以及系统原生文件夹选择对话框。
- **状态栏**：卷可用空间/总容量、文件总数、文件系统（NTFS 等）、簇大小、扫描错误统计（✓ 正常 / ✗ N 个错误，点击查看详细日志，如目录拒绝访问）。
- **自动降级**：默认尝试 USN 日志快速枚举，不可用时自动降级为并行遍历，全程无感。

---

## 📸 截图

> TODO: 添加应用界面截图（欢迎 PR）。

---

## 🚀 快速开始

### 从 Release 下载（推荐）

前往 [Releases](../../releases) 页面下载最新版本的安装包（`.msi` / `.exe`），安装后即可使用。

### 从源码构建

**环境要求**

| 依赖 | 说明 |
|---|---|
| Windows 10 / 11 | 目标平台 |
| Node.js ≥ 18 | 前端构建 + Tauri CLI |
| Rust 稳定工具链 | 后端扫描引擎 |
| WebView2 Runtime | Tauri 2 运行必需（Win11 自带，Win10 需安装） |
| Microsoft C++ Build Tools | Rust 链接 Windows 库需要 |

**开发模式**

```bash
npm install
npm run tauri dev
```

**打包发行版**

```bash
npm run tauri build    # 生成 .msi / .exe 安装包
```

**测试**

```bash
cd src-tauri
cargo check            # 编译检查
cargo test             # 真实数据单元测试（扫描 / prune / 导航 / 盘符枚举）
cargo test -- --ignored  # 性能压测（含真实 C: 盘全盘扫描，较慢）
```

> 关于 `cargo test` 在 Windows 上的 TaskDialogIndirect 崩溃问题与自动修复方案，见 [docs/接手说明-HANDOFF.md](docs/接手说明-HANDOFF.md)。

---

## 🧠 技术架构

- **前端**：React 18 + TypeScript + Vite
- **后端**：Rust —— `scanner/parallel.rs`（多线程遍历）、`scanner/prune.rs`（树裁剪）、`scanner/volinfo.rs`（卷信息）、`scanner/usn.rs`（USN 日志枚举，可选）
- **桌面壳**：Tauri 2（WebView2）
- **原生能力**：Windows API（`GetLogicalDriveStringsW` / `GetDriveTypeW` / `GetVolumeInformationW` / `GetDiskFreeSpaceExW` / `GetFileInformationByHandleEx` 等），`rfd` 提供原生文件夹选择对话框

前后端 IPC 契约见 [docs/API.md](docs/API.md)。

---

## 🤖 Agent 辅助开发

本项目由 **AI Agent（WorkBuddy）辅助开发**：需求拆解、Rust/TypeScript 实现、单元测试、性能优化（Rayon 线程池）、界面迭代与构建验证均在人类工程师的指导下由 Agent 完成。所有关键决策（架构、IPC 契约、权限模型、命名）均有书面记录并经过人工复核。

---

## 📜 开源协议

[MIT](LICENSE) © 2026 index-swf

依赖审计结论：全部 507 个 Rust 依赖与 37 个 npm 依赖均为 MIT / Apache-2.0 / BSD 等宽松协议，**不含任何 GPL 系协议**，采用 MIT 协议兼容无冲突。

---

## 📁 目录结构

```
disk-lens/
├─ src/                 # React 前端
├─ src-tauri/           # Rust 后端（Tauri 2）
│  ├─ src/scanner/      # 扫描引擎（parallel / prune / volinfo / usn）
│  ├─ src/api_tests.rs  # 真实数据单元测试 + 性能用例
│  ├─ capabilities/     # Tauri 2 命令权限（ACL）
│  ├─ tools/inject_manifest.cjs  # cargo runner：测试 EXE 注入 comctl32 v6
│  └─ .cargo/config.toml         # Windows 下挂 runner
├─ docs/                # 接口与交接文档
└─ .github/workflows/   # CI / Release 自动构建
```

---

## 📄 相关文档

- [docs/API.md](docs/API.md) —— 前后端接口契约
- [docs/接手说明-HANDOFF.md](docs/接手说明-HANDOFF.md) —— 项目交接与架构说明
