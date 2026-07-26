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

/** scan_drive 命令的返回结构（裁剪树 + 扫描统计） */
export interface ScanResult {
  root: TreeNode;
  strategy_used: "usn" | "parallel";
  elapsed_ms: number;
  total_files: number;
  total_folders: number;
  total_size: number;
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
