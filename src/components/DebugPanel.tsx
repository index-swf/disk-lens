import { useState } from "react";
import type { ScanResult } from "../types";
import { formatBytes, formatDuration } from "../utils";

interface DebugPanelProps {
  /** 是否精确获取分配大小（逐文件开句柄，更准但更慢） */
  precise: boolean;
  onPreciseChange: (b: boolean) => void;
  /** 并行线程数：0 = 自动（等于 CPU 逻辑核心数） */
  threads: number;
  onThreadsChange: (n: number) => void;
  /** 最近一次扫描结果（用于展示统计） */
  result: ScanResult | null;
  /** 是否在扫描中（用于禁用输入） */
  scanning: boolean;
}

// 可折叠调试面板（默认折叠，点击标题展开）：扫描参数 + 扫描结果统计。
export default function DebugPanel({
  precise,
  onPreciseChange,
  threads,
  onThreadsChange,
  result,
  scanning,
}: DebugPanelProps) {
  const [collapsed, setCollapsed] = useState(true);

  return (
    <section className="debug-panel">
      <div className="debug-head">
        <button
          type="button"
          className="debug-toggle"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
        >
          {collapsed ? "▶" : "▼"} 调试面板
        </button>
      </div>

      {!collapsed && (
        <div className="debug-body">
          <div className="debug-row">
            <label className="debug-check">
              <input
                type="checkbox"
                checked={precise}
                disabled={scanning}
                onChange={(e) => onPreciseChange(e.target.checked)}
              />
              精确分配大小（逐文件开句柄，更慢更准）
            </label>
            <label className="debug-label" htmlFor="threads">
              并行线程数：
            </label>
            <input
              id="threads"
              type="number"
              min={0}
              max={256}
              value={threads}
              disabled={scanning}
              onChange={(e) => onThreadsChange(Number(e.target.value) || 0)}
              className="debug-number"
            />
            <span className="debug-hint">0=自动(=CPU核心数)</span>
          </div>

          {result && (
            <div className="debug-stats">
              <span>
                实际策略：
                <b>并行遍历</b>
              </span>
              <span>
                耗时：<b>{formatDuration(result.elapsed_ms)}</b>
              </span>
              <span>
                文件：<b>{result.total_files.toLocaleString()}</b>
              </span>
              <span>
                文件夹：<b>{result.total_folders.toLocaleString()}</b>
              </span>
              <span>
                总大小：<b>{formatBytes(result.total_size)}</b>
              </span>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
