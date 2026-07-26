import type { TreeNode } from "../types";
import { formatBytes, formatPercent, isDrillable } from "../utils";

interface DataTableProps {
  /** 当前节点下的子节点列表（文件夹 + 文件聚合节点） */
  rows: TreeNode[];
  /** 当前节点的总大小，用于计算占比 */
  parentSize: number;
  /** 点击某一行时触发（若该行含子节点则下钻） */
  onRowClick: (node: TreeNode) => void;
}

// 数据表格：展示当前目录的子项。点击含子节点的行与 Treemap 下钻同步。
export default function DataTable({ rows, parentSize, onRowClick }: DataTableProps) {
  if (rows.length === 0) {
    return <div className="table-placeholder">该目录下没有子项</div>;
  }

  return (
    <div className="data-table-wrap">
      <table className="data-table">
        <thead>
          <tr>
            <th className="col-name">名称</th>
            <th className="col-num">大小</th>
            <th className="col-num">分配大小</th>
            <th className="col-num">文件数</th>
            <th className="col-num">文件夹数</th>
            <th className="col-num">占比</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, idx) => {
            const drillable = isDrillable(row);
            const ratio = parentSize > 0 ? row.size / parentSize : 0;
            return (
              <tr
                key={`${row.name}-${idx}`}
                className={drillable ? "row-drillable" : "row-leaf"}
                onClick={() => drillable && onRowClick(row)}
                title={drillable ? "点击下钻" : undefined}
              >
                <td className="col-name">{row.name}</td>
                <td className="col-num">{formatBytes(row.size)}</td>
                <td className="col-num">{formatBytes(row.allocated_size)}</td>
                <td className="col-num">{row.file_count.toLocaleString()}</td>
                <td className="col-num">{row.folder_count.toLocaleString()}</td>
                <td className="col-num">{formatPercent(ratio)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
