# USN 极速磁盘扫描器 — 项目接手说明（HANDOFF）

> 本文档由分析原 Qoder Agent 的本地记录（数据库 + 缓存）整理而成，用于接手继续开发。
> 生成日期：2026-07-26。原开发中断原因：Qoder 赠送额度用尽。

---

## 1. 项目一句话概述

构建一个 **Windows 桌面应用「USN 极速磁盘扫描器」**（类似 TreeSize Free），
核心卖点：**无需 UAC 管理员权限 + 极速扫描磁盘空间占用**，用 Treemap + 表格可视化。

- 技术栈：**Tauri 2.x（Rust 后端）+ React 18 + TypeScript + Vite（前端）+ ECharts（Treemap）**
- 扫描双引擎：优先 **USN 日志**（`windows-rs` + `FILE_FLAG_BACKUP_SEMANTICS` 免提权），失败自动降级 **`rayon` 并行目录遍历**
- 需求原文见 `feature-description.md`，完整实施计划见 `.qoder/specs/USN_Fast_Disk_Scanner_9169af05.md`

---

## 2. 原 Agent 是怎么干的（工作流还原）

原 Qoder 会话标题「从零开始搭建项目」，用户「云扬」。它采用了**多子代理协作**模式：

1. **Plan 模式调研**：并行派出 3 个研究子代理，从三个视角出方案
   - `Alex`（Plan A：简洁性/可维护性，主张 `jwalk`）
   - `Sam`（Plan B：性能/可扩展性，主张 USN 优先 + rayon 降级 + arena 分配）
   - `Jack`（Plan C：最小风险，指出 `jwalk` 版本被撤回不稳定，主张官方 `windows-rs` 直连）
2. **综合评审**：合并三方结论 → 最终采用 **USN 优先 + rayon 降级** 的双引擎方案，放弃 `jwalk`。产出实施计划（即 spec 文件）。
3. **用户批准计划** → 创建 4 个主任务（见下）。
4. **派编码子代理 `Lee`** 搭脚手架 → 被取消。
5. 用户反馈「国内下载慢」→ 改派 **`Taylor`**：先配 npm(npmmirror)/Cargo(rsproxy.cn) 镜像，再搭脚手架、`npm install`、装 Rust、`cargo check`。
6. **中断点**：`Taylor` 装完 rustup、又发现缺 MSVC 构建工具，正在 `winget install VS BuildTools` 时额度耗尽，`cargo check` 尚未跑通。

> 完整时间线（183 个操作）见 `会话操作日志.md`；对话原文见 `会话叙述.md`。

---

## 3. 任务列表与完成情况

原 Agent 用 TaskCreate 建立了 4 个主任务，依赖关系：`1 → (2,3) → 4`。
最后一次记录的状态如下：

| # | 任务 | 状态 | 说明 |
|---|------|------|------|
| 1 | 搭建 Tauri 2.x + React + Vite 项目脚手架 | 🟡 **进行中（≈90%）** | 文件已全建、依赖已装；仅差 `cargo check` 跑通（卡在 MSVC 构建工具） |
| 2 | 实现 Rust 后端扫描引擎（2A~2E） | 🔴 **未开始** | 被任务 1 阻塞 |
| 3 | 实现前端界面与数据可视化（3A~3F） | 🔴 **未开始** | 被任务 1 阻塞 |
| 4 | 集成验证、错误处理与打包 | 🔴 **未开始** | 被任务 2、3 阻塞 |

任务 1 内部 todo（Taylor）：`.npmrc` ✅ / Cargo 镜像 ✅ / 脚手架文件 ✅ / `npm install` ✅ / `cargo check` ⏳（未完成）。

---

## 4. 当前代码实际状态（已核对磁盘）

**已存在（脚手架，可运行骨架）：**
- 前端：`index.html`、`package.json`、`vite.config.ts`、`tsconfig.json`、`src/main.tsx`、`src/App.tsx`、`src/App.css`
  - ⚠️ `App.tsx` 目前只是一个「Test IPC Connection」测试按钮，**没有任何扫描 UI**
- 后端：`src-tauri/Cargo.toml`、`build.rs`、`tauri.conf.json`、`capabilities/default.json`、`src/main.rs`、`src/lib.rs`
  - ⚠️ `lib.rs` 只有一个 `greet()` hello-world 命令，**没有任何扫描逻辑**
  - ✅ `Cargo.toml` 依赖已按计划配好：`windows 0.58`（含 Win32_Storage_FileSystem / System_IO / System_Ioctl）、`rayon 1.10`、`serde`、`serde_json`、`thiserror`、`tokio`
- `.npmrc` 已配 npmmirror 镜像；`node_modules/`（45 包）与 `src-tauri/target/` 均已存在

**尚未创建（需要新写的核心代码）：**
- `src-tauri/src/models.rs`（TreeNode 数据模型）
- `src-tauri/src/scanner/mod.rs`、`usn.rs`、`parallel.rs`、`tree_builder.rs`（扫描引擎，全部缺失）
- `src/types/index.ts`、`src/components/`（TreemapChart / DataTable / Breadcrumb / ScanProgress / DriveSelector，全部缺失）

---

## 5. 接手后的下一步（建议顺序）

1. **打通环境**（完成任务 1 的最后一步）
   - 确认 Rust 已装：`C:\Users\index\.cargo\bin\cargo.exe`
   - **关键前提：安装 MSVC 构建工具**（原 Agent 卡在这里）。装 Visual Studio Build Tools 的「使用 C++ 的桌面开发」工作负载：
     `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`
   - 然后 `cd src-tauri && cargo check` 应能通过；`npm run tauri dev` 验证前后端 IPC。
2. **任务 2：Rust 后端**（按 spec 的 2A→2B/2C→2D→2E）
   - 先 `models.rs`（TreeNode），再并行遍历引擎 `parallel.rs`（先易后难，能最快出可用结果），再 USN 引擎 `usn.rs`，然后 `tree_builder.rs` 聚合，最后 `scan_drive` 命令 + 进度事件。
3. **任务 3：前端**（可用 mock JSON 先行开发）：Treemap + 表格 + 面包屑 + 进度 + 驱动器选择。
4. **任务 4**：错误处理、集成验证、`tauri.conf.json` 打包（NSIS/MSI，目标 <10MB）。

> 详细的每文件设计、代码片段、被否决的方案（如为何弃用 jwalk / MFT / Zustand）都在 spec 文件里，接手前务必通读。

---

## 6. 原始记录文件索引

| 文件 | 内容 |
|------|------|
| `feature-description.md` | 用户最初的需求文档 |
| `.qoder/specs/USN_Fast_Disk_Scanner_9169af05.md` | 完整实施计划（含技术栈、5 个 Phase、文件结构、性能目标、被否决方案） |
| `会话叙述.md` | 主会话对话气泡原文（用户与 Agent 的往来） |
| `会话操作日志.md` | 183 个工具操作的完整时间线（含所有子代理动作） |
| `接手说明-HANDOFF.md` | 本文件 |

原始数据来源（Qoder 缓存，只读，未改动）：
- 会话数据库：`C:\Users\index\AppData\Roaming\Qoder\SharedClientCache\cache\db\local.db`（含 6 个会话 / 299 条消息 / 4 个任务）
- 会话历史：`C:\Users\index\.qoder\cache\projects\tree-scan-fc5c47d\conversation-history\9169af05\9169af05.jsonl`
- Agent 记忆：`C:\Users\index\.qoder\memories\019f941d\`（技术栈/构建/环境等结构化记忆）

> 注：数据库中 `chat_message.content`（对话正文）与 `task_tree` 为加密存储，无法直接解密；
> 但工具执行日志 `tool_result` 为明文，本接手文档的操作还原即基于此，信息已足够完整。
