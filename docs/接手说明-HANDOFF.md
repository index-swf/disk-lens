# DiskLens — 项目交接与架构说明（HANDOFF）

> 面向后续维护者的技术文档。README 是用户视角的简介，本文是开发者视角的架构与约定。
> 本项目由 **AI Agent（WorkBuddy）辅助开发**，关键决策均有书面记录。

---

## 1. 项目概述

**DiskLens**：Windows 磁盘占用分析器（类似 TreeSize Free 的轻量开源替代品）。
核心卖点：**极速扫描**（多线程并行遍历，C: 盘约 274 GB / 120 万文件实测约 8 秒）+ 可折叠树形列表呈现占用分布。

- 桌面壳：**Tauri 2**（WebView2）
- 前端：React 18 + TypeScript + Vite
- 后端：Rust（`rayon` 多线程扫描引擎）
- 开源协议：**MIT**

---

## 2. 架构总览

```
React 前端 (src/)                 Rust 后端 (src-tauri/)
┌──────────────────┐   Tauri IPC   ┌─────────────────────────────┐
│ TreeTable 树列表  │ ◄───────────► │ lib.rs: scan_drive/get_node  │
│ DriveSelector     │   invoke()    │   list_drives/pick_folder    │
│ StatusBar         │               │ scanner/                     │
│ ScanProgress      │ ◄─── emit ─── │   parallel.rs  并行遍历(主)  │
└──────────────────┘   scan-progress│   prune.rs      树裁剪       │
                                    │   volinfo.rs    卷信息       │
                                    │   usn.rs        USN 日志(可选)│
                                    └─────────────────────────────┘
```

### 数据流

1. `scan_drive` 扫描整个卷/目录，构建**完整树**并缓存在后端 `AppState`；
2. 返回给前端的是一棵**裁剪树**（`prune`：深度预算 + 每层 Top N + "(其他 N 项)"聚合 + 散文件保留），控制 IPC 体积；
3. 前端树形列表按需展开，遇到 `truncated` 节点调用 `get_node` 懒加载真实子树；
4. `scan-progress` 事件流式上报扫描进度。

### 关键设计决策

| 决策 | 原因 |
|---|---|
| `merge_files=false` 时散文件保留为独立子节点 | 前端用 `N [files]` 虚拟节点统一折叠展示；曾因直接丢弃导致目录展开后空白 |
| `prune` 返回前按 size 降序 | 保持"子节点按大小降序"的全局不变式 |
| `scan_parallel` 一律用 scoped 本地 rayon 池 | 混用全局池会污染后续扫描（曾慢 ~60×） |
| 快速路径按卷簇大小向上取整算"占用分配空间" | 与资源管理器一致，且零额外 syscall |
| Tauri 2 命令级 ACL（`permissions/scan.toml` + `capabilities/default.json`） | 新增命令必须同时声明权限，否则报 `not allowed. Command not found` |
| auto 模式 USN 优先、失败降级并行遍历 | 本机实测 USN 日志不可用时自动降级，用户无感 |

---

## 3. 模块说明

### 后端（src-tauri/src）

- **`scanner/parallel.rs`**：主扫描引擎。`scan_dir` 递归 + rayon 并行；错误（如目录拒绝访问）经 `ScanCtx::record_error` 收集（总数 + 上限 1000 条明细）。
- **`scanner/prune.rs`**：树裁剪。`is_dir_child` 判定文件叶子（`file_count==1 && folder_count==0 && children.is_empty()`）；聚合 "(其他 N 项)"。
- **`scanner/volinfo.rs`**：卷信息（可用/总空间、文件系统、簇大小），用 `GetDiskFreeSpaceExW/W` + `GetVolumeInformationW`。
- **`scanner/usn.rs`**：USN 日志枚举（可选加速路径，环境不支持时降级）。
- **`lib.rs`**：Tauri 命令（`scan_drive`/`get_node`/`list_drives`/`pick_folder`）+ 全树缓存 `AppState`。
- **`api_tests.rs`**：真实数据单元测试（扫描/prune/导航/盘符枚举）+ `#[ignore]` 性能用例。

### 前端（src/）

- **`components/TreeTable.tsx`**：树形列表。名称列 = 大小(固定宽度右对齐) + 名称(左对齐)；`[files]` 虚拟节点；表头排序；懒加载。
- **`components/DriveSelector.tsx`**：三段式下拉（真实磁盘含类型/卷标、最近 5 次扫描、自定义路径原生对话框）。
- **`components/StatusBar.tsx`**：卷信息 + 扫描错误统计（✓/✗ 点击弹窗看明细）。
- **`App.tsx`**：状态编排；**调试面板仅在 `import.meta.env.DEV` 渲染，生产打包自动移除**。

---

## 4. 构建与测试

```bash
npm install
npm run tauri dev        # 开发模式
npm run tauri build      # 打包发行版（MSI / NSIS）
cd src-tauri
cargo check              # 编译检查
cargo test               # 真实数据单元测试
cargo test -- --ignored  # 性能压测（含真实 C: 盘全盘扫描）
```

> Windows 下 `cargo test` 依赖 `src-tauri/.cargo/config.toml` 的 runner 方案
> （`tools/inject_manifest.cjs` 给测试 EXE 幂等注入 comctl32 v6 manifest，规避
> TaskDialogIndirect 崩溃）。路径全部相对，项目搬迁无需改配置。

发布：打 `v*` tag 触发 `.github/workflows/release.yml`（tauri-action 自动构建并发布到 GitHub Release）。

---

## 5. 相关文档

- [`docs/API.md`](API.md) —— 前后端 IPC 契约
- [`feature-description.md`](../feature-description.md) —— 原始需求描述
- [`README.md`](../README.md) —— 用户视角简介
