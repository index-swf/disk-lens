import { useMemo } from "react";
import type { TreeNode } from "../types";
import { formatBytes, formatDate, formatPercent } from "../utils";

// ======================= 类型与判定 =======================

export type SortKey =
  | "name"
  | "size"
  | "allocated_size"
  | "file_count"
  | "folder_count"
  | "pct"
  | "last_modified";

export type SortDir = "asc" | "desc";

export type RowKind = "dir" | "file" | "files" | "aggregate";

export interface TreeRow {
  /** 从根起的唯一路径键，如 "C:/Users/index"；[files] 虚拟键为 ".../__files__" */
  key: string;
  node: TreeNode;
  depth: number;
  /** 父节点大小（用于"占父目录 %"列） */
  parentSize: number;
  kind: RowKind;
  /** 是否可展开（目录、或已加载文件的 [files] 虚拟节点） */
  expandable: boolean;
  expanded: boolean;
  /** 该节点名称列背景占比条宽度百分比（相对整盘根总大小） */
  sizePctOfRoot: number;
}

interface TreeTableProps {
  root: TreeNode;
  /** 整盘根总大小，作为背景占比条的 100% 分母 */
  rootSize: number;
  expanded: Set<string>;
  /** [files] 虚拟节点的展开集合 */
  filesExpanded: Set<string>;
  /** path key -> 通过 get_node 懒加载得到的子树（覆盖裁剪版 children） */
  loaded: Map<string, TreeNode>;
  sortKey: SortKey;
  sortDir: SortDir;
  onToggleDir: (key: string) => void;
  onToggleFiles: (key: string) => void;
  onSort: (key: SortKey) => void;
}

// 聚合节点（"(其他 N 项)"）不可下钻，只是被裁剪子项的汇总。
function isAggregateNode(n: TreeNode): boolean {
  return n.name.startsWith("(其他");
}

// 真正的"文件叶子"：本模型里只有 file_count==1 && folder_count==0 && 无子节点
// 的节点才是单个文件；其余（哪怕 folder_count==0 但含多文件）都是目录。
function isFileNode(n: TreeNode): boolean {
  return (
    !isAggregateNode(n) &&
    n.file_count === 1 &&
    n.folder_count === 0 &&
    n.children.length === 0
  );
}

function isDirNode(n: TreeNode): boolean {
  return !isAggregateNode(n) && !isFileNode(n);
}

// ======================= 行构建 =======================

function sumSizes(nodes: TreeNode[]): number {
  return nodes.reduce((s, n) => s + n.size, 0);
}

function sortNodes(
  nodes: TreeNode[],
  key: SortKey,
  dir: SortDir,
  parentSize: number
): TreeNode[] {
  const out = [...nodes];
  const mul = dir === "asc" ? 1 : -1;
  out.sort((a, b) => {
    let cmp = 0;
    switch (key) {
      case "name":
        cmp = a.name.localeCompare(b.name, "zh-Hans-CN");
        break;
      case "size":
        cmp = a.size - b.size;
        break;
      case "allocated_size":
        cmp = a.allocated_size - b.allocated_size;
        break;
      case "file_count":
        cmp = a.file_count - b.file_count;
        break;
      case "folder_count":
        cmp = a.folder_count - b.folder_count;
        break;
      case "last_modified":
        cmp = a.last_modified - b.last_modified;
        break;
      case "pct":
        cmp =
          parentSize > 0 ? a.size / parentSize - b.size / parentSize : 0;
        break;
    }
    return cmp * mul;
  });
  return out;
}

// 构造 [files] 虚拟节点（仅用于展示/占比计算）。
// 只统计【当前目录直接包含的文件】(children 中已是文件叶子的项)，
// 不递归子目录——避免出现 "903783 [files]" 这种巨型列表。
// 注意:后端 prune 在 merge_files=false 时不会裁剪直接文件(只有子目录受
// topN 限制)，所以 children 里的文件叶子就是该目录直接文件的完整集合。
function makeFilesVirtual(files: TreeNode[]): TreeNode {
  const size = sumSizes(files);
  const allocated = files.reduce((s, f) => s + f.allocated_size, 0);
  return {
    name: `${files.length} [files]`,
    size,
    allocated_size: allocated,
    file_count: files.length,
    folder_count: 0,
    last_modified: 0,
    children: [],
    truncated: false,
  };
}

function buildRows(
  root: TreeNode,
  rootSize: number,
  expanded: Set<string>,
  filesExpanded: Set<string>,
  loaded: Map<string, TreeNode>,
  sortKey: SortKey,
  sortDir: SortDir,
  parentKey: string,
  node: TreeNode,
  depth: number,
  parentSize: number,
  rows: TreeRow[]
): void {
  // Linux 绝对路径下拼接会产生 "//home" 双斜杠；规范化成单斜杠，保证 key
  // 与前端 findNode / 后端 get_node 的路径解析一致。
  const key = parentKey
    ? `${parentKey}/${node.name}`.replace(/\/+/g, "/")
    : node.name;
  // 若已通过 get_node 懒加载到更完整的子树，优先用加载版。
  const effective = loaded.get(key) ?? node;
  const isRoot = depth === 0;

  rows.push({
    key,
    node: effective,
    depth,
    parentSize: isRoot ? effective.size : parentSize,
    kind: isRoot
      ? "dir"
      : isFileNode(effective)
        ? "file"
        : isAggregateNode(effective)
          ? "aggregate"
          : "dir",
    expandable: false, // 由下方子节点逻辑决定
    expanded: expanded.has(key),
    sizePctOfRoot: rootSize > 0 ? (effective.size / rootSize) * 100 : 0,
  });

  // 折叠则不再展开子层。
  if (!expanded.has(key)) return;

  const dirs = effective.children.filter(isDirNode);
  const files = effective.children.filter(isFileNode);
  const aggregates = effective.children.filter(isAggregateNode);

  const sortedDirs = sortNodes(dirs, sortKey, sortDir, effective.size);
  const sortedFiles = sortNodes(files, sortKey, sortDir, effective.size);
  const sortedAgg = sortNodes(aggregates, sortKey, sortDir, effective.size);

  // 1) 子目录
  for (const d of sortedDirs) {
    buildRows(
      root,
      rootSize,
      expanded,
      filesExpanded,
      loaded,
      sortKey,
      sortDir,
      key,
      d,
      depth + 1,
      effective.size,
      rows
    );
  }

  // 2) [files] 虚拟节点：只要该目录【直接】包含文件（且本身不是单文件）就显示。
  //    数量/大小只统计当前目录的直接文件，不递归子目录。
  if (files.length > 0) {
    const fkey = `${key}/__files__`;
    const fExpanded = filesExpanded.has(fkey);
    const virtual = makeFilesVirtual(files);
    rows.push({
      key: fkey,
      node: virtual,
      depth: depth + 1,
      parentSize: effective.size,
      kind: "files",
      expandable: files.length > 0,
      expanded: fExpanded,
      sizePctOfRoot: rootSize > 0 ? (virtual.size / rootSize) * 100 : 0,
    });
    if (fExpanded) {
      for (const f of sortedFiles) {
        buildRows(
          root,
          rootSize,
          expanded,
          filesExpanded,
          loaded,
          sortKey,
          sortDir,
          fkey,
          f,
          depth + 2,
          virtual.size,
          rows
        );
      }
    }
  }

  // 3) 聚合节点（"(其他 N 项)"）—— 不可下钻，仅展示。
  for (const a of sortedAgg) {
    rows.push({
      key: `${key}/__agg__${a.name}`,
      node: a,
      depth: depth + 1,
      parentSize: effective.size,
      kind: "aggregate",
      expandable: false,
      expanded: false,
      sizePctOfRoot: rootSize > 0 ? (a.size / rootSize) * 100 : 0,
    });
  }
}

// ======================= 图标 =======================

function iconFor(kind: RowKind, name: string): string {
  if (kind === "file") return name.toLowerCase().endsWith(".lnk") ? "🔗" : "📄";
  if (kind === "files") return "📄";
  if (kind === "aggregate") return "🗂️";
  return "📂"; // dir
}

// ======================= 组件 =======================

const COLUMNS: { key: SortKey; label: string; numeric: boolean }[] = [
  { key: "name", label: "名称", numeric: false },
  { key: "allocated_size", label: "占用分配空间", numeric: true },
  { key: "file_count", label: "文件数", numeric: true },
  { key: "folder_count", label: "目录数", numeric: true },
  { key: "pct", label: "占父目录 %", numeric: true },
  { key: "last_modified", label: "最后修改日期", numeric: true },
];

export default function TreeTable({
  root,
  rootSize,
  expanded,
  filesExpanded,
  loaded,
  sortKey,
  sortDir,
  onToggleDir,
  onToggleFiles,
  onSort,
}: TreeTableProps) {
  const rows = useMemo(() => {
    const out: TreeRow[] = [];
    buildRows(
      root,
      rootSize,
      expanded,
      filesExpanded,
      loaded,
      sortKey,
      sortDir,
      "",
      root,
      0,
      root.size,
      out
    );
    return out;
  }, [root, rootSize, expanded, filesExpanded, loaded, sortKey, sortDir]);

  return (
    <div className="tree-table-wrap">
      <table className="tree-table">
        <thead>
          <tr>
            {COLUMNS.map((c) => (
              <th
                key={c.key}
                className={c.numeric ? "col-num" : "col-name"}
                onClick={() => onSort(c.key)}
                aria-sort={
                  sortKey === c.key
                    ? sortDir === "asc"
                      ? "ascending"
                      : "descending"
                    : "none"
                }
              >
                <span className="th-label">{c.label}</span>
                <span className="th-sort">
                  {sortKey === c.key ? (sortDir === "asc" ? "▲" : "▼") : "↕"}
                </span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const clickable = r.kind === "dir" || r.kind === "files";
            const pct =
              r.parentSize > 0
                ? (r.node.size / r.parentSize) * 100
                : r.kind === "dir" && r.depth === 0
                  ? 100
                  : 0;
            // 名称列布局：大小（固定宽度、右对齐）在前，名称（左对齐）在后；
            // [files] 虚拟节点本身已是 "N [files]"；文件/聚合直接显示原名。
            const displayName = r.node.name;
            return (
              <tr key={r.key} className={`row-${r.kind}`}>
                <td className="col-name">
                  <div
                    className="name-cell"
                    style={{ paddingLeft: `${r.depth * 18 + 6}px` }}
                  >
                    {/* 背景占比条：相对整盘根总大小 */}
                    <div
                      className="name-bar"
                      style={{ width: `${Math.min(100, r.sizePctOfRoot)}%` }}
                    />
                    {/* 折叠/展开三角 */}
                    <span
                      className={
                        "twisty " +
                        (clickable ? "twisty-on" : "twisty-off") +
                        (r.expanded ? " twisty-open" : "")
                      }
                      onClick={
                        clickable
                          ? () =>
                              r.kind === "dir"
                                ? onToggleDir(r.key)
                                : onToggleFiles(r.key)
                          : undefined
                      }
                    >
                      {clickable ? (r.expanded ? "▼" : "▶") : ""}
                    </span>
                    <span className="node-icon">{iconFor(r.kind, r.node.name)}</span>
                    {/* 大小前置：固定最小宽度 + 右对齐，不足宽度时左侧留白 */}
                    <span className="node-size">{formatBytes(r.node.size)}</span>
                    <span className="node-name">{displayName}</span>
                  </div>
                </td>
                <td className="col-num">{formatBytes(r.node.allocated_size)}</td>
                <td className="col-num">{r.node.file_count.toLocaleString()}</td>
                <td className="col-num">{r.node.folder_count.toLocaleString()}</td>
                <td className="col-num col-pct">
                  {/* 占父目录 %：文字 + 单元格背景进度条 */}
                  <div className="pct-cell">
                    <div
                      className="pct-bar"
                      style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
                    />
                    <span className="pct-text">{formatPercent(pct / 100)}</span>
                  </div>
                </td>
                <td className="col-num">{formatDate(r.node.last_modified)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
