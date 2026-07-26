import { useState } from "react";
import type { ScanMethod, ScanResult, TopNMode } from "../types";
import { formatBytes, formatDuration } from "../utils";

interface DebugPanelProps {
  /** 当前扫描方式 */
  method: ScanMethod;
  onMethodChange: (m: ScanMethod) => void;
  /** 返回树最大深度 */
  maxDepth: number;
  onMaxDepthChange: (n: number) => void;
  /** 每层 Top N / 百分比阈值 */
  topN: number;
  onTopNChange: (n: number) => void;
  /** topN 含义模式：count=数量 / percent=百分比覆盖 */
  topNMode: TopNMode;
  onTopNModeChange: (m: TopNMode) => void;
  /** 是否将目录下所有文件合并为单个"(N 个文件)"节点（仿 TreeSize） */
  mergeFiles: boolean;
  onMergeFilesChange: (b: boolean) => void;
  /** 是否精确获取分配大小（逐文件开句柄，更准但更慢） */
  precise: boolean;
  onPreciseChange: (b: boolean) => void;
  /** 并行线程数：0 = 自动（等于 CPU 逻辑核心数） */
  threads: number;
  onThreadsChange: (n: number) => void;
  /** 是否使用 mock 数据（原 App 顶部的开关已挪入此处） */
  useMock: boolean;
  onUseMockChange: (b: boolean) => void;
  /** 最近一次扫描结果（用于展示统计） */
  result: ScanResult | null;
  /** 是否在扫描中（用于禁用输入） */
  scanning: boolean;
}

const METHOD_LABELS: Record<ScanMethod, string> = {
  auto: "自动（USN 优先）",
  usn: "强制 USN",
  parallel: "强制并行遍历",
};

// 可折叠调试面板（默认展开）：扫描参数调节 + 扫描结果统计。
export default function DebugPanel({
  method,
  onMethodChange,
  maxDepth,
  onMaxDepthChange,
  topN,
  onTopNChange,
  topNMode,
  onTopNModeChange,
  mergeFiles,
  onMergeFilesChange,
  precise,
  onPreciseChange,
  threads,
  onThreadsChange,
  useMock,
  onUseMockChange,
  result,
  scanning,
}: DebugPanelProps) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <section className="debug-panel">
      <div className="debug-head">
        <button
          type="button"
          className="debug-toggle"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
        >
          {collapsed ? "▶" : "▼"} 调试面板 / 扫描参数
        </button>
      </div>

      {!collapsed && (
        <div className="debug-body">
          <div className="debug-row">
            <span className="debug-label">扫描方式：</span>
            <div className="debug-radios">
              {(Object.keys(METHOD_LABELS) as ScanMethod[]).map((m) => (
                <label key={m} className="debug-radio">
                  <input
                    type="radio"
                    name="scan-method"
                    value={m}
                    checked={method === m}
                    disabled={scanning}
                    onChange={() => onMethodChange(m)}
                  />
                  {METHOD_LABELS[m]}
                </label>
              ))}
            </div>
          </div>

          <div className="debug-row">
            <label className="debug-label" htmlFor="max-depth">
              返回深度 maxDepth：
            </label>
            <input
              id="max-depth"
              type="number"
              min={1}
              max={10}
              value={maxDepth}
              disabled={scanning}
              onChange={(e) => onMaxDepthChange(Number(e.target.value) || 1)}
              className="debug-number"
            />
            <label className="debug-label" htmlFor="topn-mode">
              Top 模式：
            </label>
            <select
              id="topn-mode"
              value={topNMode}
              disabled={scanning}
              onChange={(e) => onTopNModeChange(e.target.value as TopNMode)}
              className="debug-select"
            >
              <option value="count">每层最多 N 项</option>
              <option value="percent">覆盖父级 N% 大小</option>
            </select>
            <label className="debug-label" htmlFor="top-n">
              {topNMode === "percent" ? "百分比 %：" : "每层 Top N："}
            </label>
            <input
              id="top-n"
              type="number"
              min={topNMode === "percent" ? 1 : 1}
              max={topNMode === "percent" ? 100 : 1000}
              value={topN}
              disabled={scanning}
              onChange={(e) => onTopNChange(Number(e.target.value) || 1)}
              className="debug-number"
            />
          </div>

          <div className="debug-row">
            <label className="debug-check">
              <input
                type="checkbox"
                checked={mergeFiles}
                disabled={scanning}
                onChange={(e) => onMergeFilesChange(e.target.checked)}
              />
              合并目录下文件为一项
            </label>
            <label className="debug-check">
              <input
                type="checkbox"
                checked={precise}
                disabled={scanning}
                onChange={(e) => onPreciseChange(e.target.checked)}
              />
              精确分配大小（慢）
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

          <div className="debug-row">
            <label className="debug-check">
              <input
                type="checkbox"
                checked={useMock}
                disabled={scanning}
                onChange={(e) => onUseMockChange(e.target.checked)}
              />
              使用 mock 数据
            </label>
          </div>

          {result && (
            <div className="debug-stats">
              <span>
                实际策略：
                <b>{result.strategy_used === "usn" ? "USN 日志" : "并行遍历"}</b>
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
