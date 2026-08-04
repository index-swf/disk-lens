// 与后端 Rust 模型严格对齐的接口定义。字段名/类型需与后端序列化结果一致。

/** 扫描策略：auto=自动(USN优先失败降级并行) / usn=强制USN / parallel=强制并行遍历 */
export type ScanMethod = "auto" | "usn" | "parallel";

/** topN 含义模式：count=每层最多 N 个项目 / percent=覆盖父级 N% 大小为止 */
export type TopNMode = "count" | "percent";

/** 目录树节点（对应后端 TreeNode；可裁剪） */
export interface TreeNode {
  name: string;
  size: number;
  allocated_size: number;
  file_count: number;
  folder_count: number;
  /** 最后修改时间，Unix 秒（UTC）；0 表示未知 */
  last_modified: number;
  children: TreeNode[];
  /**
   * 该节点是否为"被后端裁剪"的节点：
   * - true 表示其 children 因超过 maxDepth 而被置空（需在 UI 层按需 get_node 取子树）；
   * - 聚合节点"(其他 N 项)"也可能带 truncated，但因其无法被 get_node 定位而禁止下钻。
   */
  truncated?: boolean;
}

/** 扫描进度事件 payload（对应后端 emit("scan-progress", ...)） */
export interface ScanProgress {
  scanned_files: number;
  scanned_folders: number;
  current_dir: string;
}

/** 扫描过程中收集的错误(如目录拒绝访问)。count 为总数,samples 为截断的明细 */
export interface ScanErrors {
  count: number;
  samples: string[];
}

/** 本机一个逻辑磁盘（对应后端 DriveInfo；list_drives 返回） */
export interface DriveInfo {
  /** 盘符，如 "C:" */
  letter: string;
  /** 卷标，可能为空 */
  label: string;
  /** 磁盘类型：本地磁盘 / 可移动磁盘 / 光驱 / 网络驱动器 / RAM 磁盘 / 未知类型 */
  kind: string;
}

/** scan_drive 命令的返回结构（裁剪树 + 扫描统计 + 卷信息 + 错误日志） */
export interface ScanResult {
  root: TreeNode;
  strategy_used: "usn" | "parallel";
  elapsed_ms: number;
  total_files: number;
  total_folders: number;
  total_size: number;
  /** 卷可用空间（字节） */
  free_bytes: number;
  /** 卷总容量（字节） */
  total_bytes: number;
  /** 文件系统名，如 "NTFS" */
  fs_type: string;
  /** 每簇字节数（如 4096）；0 = 未知 */
  cluster_size: number;
  /** 扫描错误（访问拒绝等） */
  errors: ScanErrors;
}

/** 导航栈中的一项：节点 + 从根到该节点的 name 路径（根为 []）+ 在父级中的大小占比 */
export interface NavItem {
  node: TreeNode;
  /** 从根节点往下的子节点 name 数组，不含根本身；根节点为空数组 [] */
  path: string[];
  /** 该节点在父级中的大小占比（0~1），根节点为 1 */
  ratio: number;
}

/** 字节数格式化选项 */
export interface FormatOptions {
  decimals?: number;
}
