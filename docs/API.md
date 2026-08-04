# 前后端交互接口（IPC 契约）

DiskLens 是 **Tauri 2** 应用：Rust 后端通过 Tauri IPC 暴露命令，React 前端用
`@tauri-apps/api/core` 的 `invoke()` 调用，进度通过 `listen()` 订阅事件。

> 所有命令参数在前端用 **camelCase** 传，Tauri 自动映射为后端的 **snake_case**。
> 本文档以「前端参数名（后端字段）」同时标注。

---

## 1. 命令 `scan_drive` —— 扫描整卷并返回裁剪树

**前端调用**
```ts
const result = await invoke<ScanResult>("scan_drive", {
  drivePath,        // string，必填
  method,           // "auto" | "usn" | "parallel"
  maxDepth,         // number
  topN,             // number
  topNMode,         // "count" | "percent"
  mergeFiles,       // boolean
  precise,          // boolean
  threads,          // number | null
});
```

**请求参数**

| 前端参数 | 后端字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|---|
| `drivePath` | `drive_path` | string | 无（必填） | 卷路径，如 `C:` / `C:\` / `D:\foo`；`C:` 会被规范为 `C:\` |
| `method` | `method` | string | `"auto"` | `auto`=NTFS 先试 USN，失败降级并行；`usn`=强制 USN，失败直接报错；`parallel`=强制并行遍历 |
| `maxDepth` | `max_depth` | u32 | `4` | 返回树向下展开层数（首屏深度）；超出的子节点 `children` 置空并标 `truncated` |
| `topN` | `top_n` | u32 | `100` | 每目录保留的最大子项数量；`topNMode=percent` 时含义变为「覆盖父级 N% 大小」 |
| `topNMode` | `top_n_mode` | string | `"count"` | `count`=按数量截断；`percent`=按累计大小占比截断（仿 TreeSize） |
| `mergeFiles` | `merge_files` | bool | `false` | 为 `true` 时把目录下所有散文件折叠成单个「(N 个文件)」节点 |
| `precise` | `precise` | bool | `false` | 为 `true` 时逐个文件开句柄取「分配大小(allocated_size)」；更准但更慢 |
| `threads` | `threads` | u32\|null | `null`(=0=自动) | 并行遍历线程数；`0`/null = 用满 CPU 核心数 |

**响应 `ScanResult`**
```ts
interface ScanErrors {
  count: number;            // 扫描错误总数（如目录拒绝访问）
  samples: string[];        // 明细日志（最多 1000 条）
}

interface ScanResult {
  root: TreeNode;            // 已裁剪的树（首屏可直接渲染）
  strategy_used: "usn" | "parallel";  // 实际使用的扫描策略
  elapsed_ms: number;        // 扫描+建树耗时（不含前端渲染/IPC 序列化）
  total_files: number;       // 全盘文件总数
  total_folders: number;     // 全盘文件夹总数
  total_size: number;        // 全盘总大小（字节，不丢）
  free_bytes: number;        // 卷可用空间（字节）；0=不可用
  total_bytes: number;       // 卷总容量（字节）；0=不可用
  fs_type: string;           // 文件系统名，如 "NTFS"；空=不可用
  cluster_size: number;      // 每簇字节数（如 4096）；0=未知
  errors: ScanErrors;        // 扫描错误（访问拒绝等）
}
```

**副作用**：扫描过程中持续 `emit("scan-progress", ...)`（见事件 3）。

**错误**：返回 `String`（前端 `catch` 拿到字符串）。例如路径不存在：
`path does not exist: X`。

---

## 2. 命令 `get_node` —— 按需展开某个子目录

首屏只拿到裁剪树；点击被折叠的目录时，前端按 `path` 从后端缓存的**完整树**取子树。

**前端调用**
```ts
const subtree = await invoke<TreeNode>("get_node", {
  path,          // string[]，必填
  maxDepth,      // number
  topN,          // number
  topNMode,      // "count" | "percent"
  mergeFiles,    // boolean
});
```

**请求参数**

| 前端参数 | 后端字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|---|
| `path` | `path` | `Vec<String>` | 无（必填） | 从根向下的**子节点名**序列；`[]` = 根本身 |
| `maxDepth` | `max_depth` | u32 | `2` | 该节点向下再展开几层 |
| `topN` / `topNMode` / `mergeFiles` | 同上 | — | `100` / `count` / `false` | 与 `scan_drive` 语义一致 |

**响应**：单个 `TreeNode`（已按参数裁剪）。

**错误**：
- 未先 `scan_drive`：`no scan result cached; run scan_drive first`
- 路径中某层名字匹配不到（如 USN 漏扫导致树不完整）：
  `node not found: <name> 可用子节点示例: [...]`

> 合成聚合节点「(其他 N 项)」/「(N 个文件)」无法被 `get_node` 定位，属预期。

---

## 3. 命令 `list_drives` / `pick_folder` —— 扫描目标选择

**`list_drives`**：枚举本机真实存在的逻辑磁盘（`GetLogicalDriveStringsW`），含类型与卷标。

```ts
const drives = await invoke<DriveInfo[]>("list_drives");
// [{ letter: "C:", label: "System", kind: "本地磁盘" }, ...]
```

**`pick_folder`**：弹出 Windows 原生文件夹选择对话框（rfd / IFileDialog，以主窗口为父窗口），返回所选路径；取消返回 `null`。

```ts
const folder = await invoke<string | null>("pick_folder");
```

> 两个命令均为应用自定义命令，须在 `permissions/scan.toml`（`allow-list-drives`/`allow-pick-folder`）与 `capabilities/default.json` 中授权，否则前端报 `not allowed. Command not found`。

---

## 4. 事件 `scan-progress` —— 扫描进度

后端在扫描中节流（约 150ms 一次）向前端推送。

**前端订阅**
```ts
import { listen } from "@tauri-apps/api/event";
await listen<ScanProgress>("scan-progress", (e) => { /* e.payload */ });
```

**载荷 `ScanProgress`**
```ts
interface ScanProgress {
  scanned_files: number;
  scanned_folders: number;
  current_dir: string;   // 当前正在扫描的目录
}
```

---

## 4. 公共数据结构

```ts
interface TreeNode {
  name: string;
  size: number;            // 逻辑大小（字节）
  allocated_size: number;  // 分配大小（字节，precise=true 时准确；否则=size）
  file_count: number;      // 该节点下（含子孙）文件总数
  folder_count: number;    // 该节点下（含子孙）文件夹总数
  children: TreeNode[];    // 始终存在（叶子为空数组，不会省略）
  truncated: boolean;      // true=子节点被裁剪/折叠，前端需按需 get_node
}
```

---

## 5. 前端调用点（代码索引）

| 调用 | 位置 | 备注 |
|---|---|---|
| `invoke("scan_drive", {...})` | `src/App.tsx` `handleScan` | 真实扫描（mock 已移除） |
| `invoke("get_node", { path, ... })` | `src/App.tsx` `onToggleDir` | 展开 `truncated` 目录时懒加载子树 |
| `listen("scan-progress", ...)` | `src/components/ScanProgress.tsx` | 挂载时注册一次 |
