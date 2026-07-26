import { useMemo } from "react";
import ReactECharts from "echarts-for-react";
import type { EChartsOption } from "echarts";
import type { TreeNode } from "../types";
import { formatBytes, isDrillable } from "../utils";

interface TreemapChartProps {
  /** 当前下钻所在的节点（其 children 会渲染为 treemap） */
  currentNode: TreeNode | null;
  /** 点击含有子节点的矩形时触发的下钻回调 */
  onDrill: (node: TreeNode) => void;
}

// 将 TreeNode 递归转换为 ECharts treemap 数据，并保留原始节点引用便于点击下钻。
// 防御性：超过 MAX_TREEMAP_DEPTH 层后不再递归 children，避免极端情况下整棵大子树被
// 复制进 WebView 导致内存耗尽（即便后端未充分裁剪也能保证渲染安全）。
interface TreemapDatum {
  name: string;
  value: number;
  _node: TreeNode;
  children: TreemapDatum[];
}

const MAX_TREEMAP_DEPTH = 3;

function toTreemapData(node: TreeNode, depth = 0): TreemapDatum {
  const children =
    depth < MAX_TREEMAP_DEPTH ? node.children.map((c) => toTreemapData(c, depth + 1)) : [];
  return {
    name: node.name,
    value: node.size,
    // 自定义字段，供点击事件取回原始 TreeNode
    _node: node,
    children,
  };
}

export default function TreemapChart({ currentNode, onDrill }: TreemapChartProps) {
  const option = useMemo<EChartsOption | null>(() => {
    if (!currentNode) return null;

    const data = [toTreemapData(currentNode)];

    // 计算用于色阶的最大值（取根节点自身大小即可，保证全局一致）
    const maxValue = Math.max(currentNode.size, 1);

    return {
      tooltip: {
        formatter: (info: any) => {
          const n: TreeNode | undefined = info.data?._node;
          if (!n) return info.name;
          return `${n.name}<br/>大小：${formatBytes(n.size)}<br/>分配：${formatBytes(
            n.allocated_size
          )}<br/>文件：${n.file_count} · 文件夹：${n.folder_count}`;
        },
      },
      visualMap: {
        type: "continuous",
        min: 0,
        max: maxValue,
        dimension: 0,
        seriesIndex: 0,
        show: false,
        // 高对比热力色阶（浅黄 -> 橙 -> 深红），大目录越"热"越红，直观且对比强烈。
        inRange: {
          color: ["#fff2cc", "#fdb462", "#fb8b24", "#e34a33", "#b30000"],
        },
      },
      series: [
        {
          type: "treemap",
          // 限制可视深度为 3 层，避免百万级节点卡顿
          leafDepth: 3,
          roam: true,
          nodeClick: false, // 自行处理点击（统一走 onDrill）
          data,
          label: {
            show: true,
            // 白字 + 黑色描边：无论底色深浅（浅黄或深红）都清晰可读。
            color: "#ffffff",
            textBorderColor: "rgba(0,0,0,0.7)",
            textBorderWidth: 2,
            formatter: (params: any) => {
              const n: TreeNode | undefined = params.data?._node;
              const size = n ? formatBytes(n.size) : "";
              return `${params.name}\n${size}`;
            },
          },
          levels: [
            {
              itemStyle: { borderWidth: 2, borderColor: "#fff", gapWidth: 2 },
            },
            {
              itemStyle: { borderWidth: 1, borderColor: "#fff", gapWidth: 1 },
            },
            {
              itemStyle: { borderWidth: 1, borderColor: "#fff", gapWidth: 1 },
            },
          ],
          breadcrumb: { show: false },
        },
      ],
    };
  }, [currentNode]);

  if (!option) {
    return <div className="treemap-placeholder">请选择一个驱动器并开始扫描</div>;
  }

  const onEvents = {
    click: (params: any) => {
      const node: TreeNode | undefined = params.data?._node;
      // 可下钻节点（含 truncated 但需 get_node 取子树的目录）才触发下钻
      if (node && isDrillable(node)) {
        onDrill(node);
      }
    },
  };

  return (
    <ReactECharts
      key={currentNode?.name ?? "root"}
      option={option}
      onEvents={onEvents}
      style={{ width: "100%", height: "100%" }}
      notMerge
      opts={{ renderer: "canvas" }}
    />
  );
}
