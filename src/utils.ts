import type { FormatOptions, TreeNode } from "./types";

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** 聚合节点前缀：后端把"超出 topN 的其余子项"合并为此类虚拟节点，无法被 get_node 定位 */
export const AGGREGATE_PREFIX = "(其他";

/**
 * 判断某节点是否可下钻：
 * - 聚合节点（"(其他 N 项)"）不可下钻；
 * - 没有子目录（folder_count===0）的节点不可下钻——这同时挡掉了"文件合并"节点
 *   "(N 个文件)"，避免点击它去触发注定失败的 get_node；
 * - 仍含有子文件夹（folder_count>0）且（已带 children 或 truncated）的节点可下钻。
 */
export function isDrillable(node: TreeNode): boolean {
  if (node.name.startsWith(AGGREGATE_PREFIX)) return false;
  if (node.folder_count === 0) return false;
  return node.children.length > 0 || node.truncated === true;
}

/** 将字节数格式化为人类可读字符串，例如 1536 -> "1.5 KB"（默认保留 1 位小数） */
export function formatBytes(bytes: number, options: FormatOptions = {}): string {
  const { decimals = 1 } = options;
  if (bytes === 0) return "0 B";
  if (!Number.isFinite(bytes) || bytes < 0) return "-";
  const k = 1024;
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), UNITS.length - 1);
  const value = bytes / Math.pow(k, i);
  return `${value.toFixed(decimals)} ${UNITS[i]}`;
}

/** 将 0~1 的比例格式化为百分比字符串，例如 0.1234 -> "12.34%" */
export function formatPercent(ratio: number, decimals = 2): string {
  if (!Number.isFinite(ratio)) return "-";
  return `${(ratio * 100).toFixed(decimals)}%`;
}

/** 将毫秒格式化为可读时长，例如 1234 -> "1.23 s" */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "-";
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** 将 Unix 秒（后端 last_modified）格式化为 "YYYY-MM-DD HH:mm"；0/非法显示 "—" */
export function formatDate(ts: number): string {
  if (!ts || ts <= 0) return "—";
  const d = new Date(ts * 1000);
  if (isNaN(d.getTime())) return "—";
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
