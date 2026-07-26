import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  NavItem,
  ScanMethod,
  ScanProgress as ScanProgressType,
  ScanResult,
  TopNMode,
  TreeNode,
} from "./types";
import { mockTree } from "./mockData";
import { isDrillable } from "./utils";
import DriveSelector from "./components/DriveSelector";
import ScanProgress from "./components/ScanProgress";
import Breadcrumb from "./components/Breadcrumb";
import TreemapChart from "./components/TreemapChart";
import DataTable from "./components/DataTable";
import DebugPanel from "./components/DebugPanel";
import "./App.css";

export default function App() {
  const [drivePath, setDrivePath] = useState("C:");
  const [method, setMethod] = useState<ScanMethod>("auto");
  const [maxDepth, setMaxDepth] = useState(4);
  const [topN, setTopN] = useState(100);
  const [topNMode, setTopNMode] = useState<TopNMode>("count");
  const [mergeFiles, setMergeFiles] = useState(false);
  const [precise, setPrecise] = useState(false);
  const [threads, setThreads] = useState(0);
  const [useMock, setUseMock] = useState(true);

  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [navStack, setNavStack] = useState<NavItem[]>([]);
  const [scanning, setScanning] = useState(false);
  const [drilling, setDrilling] = useState(false);
  const [progress, setProgress] = useState<ScanProgressType | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleScan = useCallback(async () => {
    setScanning(true);
    setError(null);
    setScanResult(null);
    setNavStack([]);
    setProgress(null);

    try {
      let result: ScanResult;
      if (useMock) {
        // mock 模式：复用本地完整树，包装成 ScanResult 形状（无需真实后端）
        result = {
          root: mockTree,
          strategy_used: "parallel",
          elapsed_ms: 1234,
          total_files: mockTree.file_count,
          total_folders: mockTree.folder_count,
          total_size: mockTree.size,
        };
      } else {
        // 真实 IPC：按新契约调用 scan_drive，后端返回裁剪树 + 统计。
        result = await invoke<ScanResult>("scan_drive", {
          drivePath,
          method,
          maxDepth,
          topN,
          topNMode,
          mergeFiles,
          precise,
          threads: threads > 0 ? threads : null,
        });
      }
      setScanResult(result);
      setNavStack([{ node: result.root, path: [], ratio: 1 }]);
    } catch (e) {
      setError(typeof e === "string" ? e : JSON.stringify(e));
    } finally {
      setScanning(false);
    }
  }, [useMock, drivePath, method, maxDepth, topN]);

  const handleDrill = useCallback(
    async (node: TreeNode) => {
      // 不可下钻（含聚合节点、纯文件叶节点）→ 直接忽略
      if (!isDrillable(node)) return;

      const parent = navStack[navStack.length - 1];
      const parentSize = parent ? parent.node.size : node.size;
      const newPath = (parent?.path ?? []).concat(node.name);

      // mock 模式下直接入栈（本地树已含全部 children）
      // 真实模式下，若节点已带 children（后端未裁剪）也直接入栈
      if (useMock || node.children.length > 0) {
        setNavStack((prev) => [
          ...prev,
          {
            node,
            path: newPath,
            ratio: parentSize > 0 ? node.size / parentSize : 0,
          },
        ]);
        return;
      }

      // 否则按需从后端拉取该路径的子树（truncated 或 folder_count>0 但 children 为空）
      setDrilling(true);
      try {
        const subtree = await invoke<TreeNode>("get_node", {
          path: newPath,
          maxDepth,
          topN,
          topNMode,
          mergeFiles,
        });
        setNavStack((prev) => [
          ...prev,
          {
            node: subtree,
            path: newPath,
            ratio: parentSize > 0 ? subtree.size / parentSize : 0,
          },
        ]);
      } catch (e) {
        setError(typeof e === "string" ? e : JSON.stringify(e));
      } finally {
        setDrilling(false);
      }
    },
    [navStack, useMock, maxDepth, topN]
  );

  const handleNavigate = useCallback((index: number) => {
    setNavStack((prev) => prev.slice(0, index + 1));
  }, []);

  const handleProgress = useCallback((p: ScanProgressType) => {
    setProgress(p);
  }, []);

  const currentItem = navStack[navStack.length - 1];
  const currentNode = currentItem?.node ?? null;
  const rows = currentNode?.children ?? [];
  const parentSize = currentNode?.size ?? 0;

  return (
    <div className="app">
      <header className="app-header">
        <h1 className="app-title">USN 极速磁盘扫描器</h1>
        <DriveSelector value={drivePath} onChange={setDrivePath} disabled={scanning} />
        <button className="scan-btn" onClick={handleScan} disabled={scanning}>
          {scanning ? "扫描中…" : "开始扫描"}
        </button>
      </header>

      <DebugPanel
        method={method}
        onMethodChange={setMethod}
        maxDepth={maxDepth}
        onMaxDepthChange={setMaxDepth}
        topN={topN}
        onTopNChange={setTopN}
        topNMode={topNMode}
        onTopNModeChange={setTopNMode}
        mergeFiles={mergeFiles}
        onMergeFilesChange={setMergeFiles}
        precise={precise}
        onPreciseChange={setPrecise}
        threads={threads}
        onThreadsChange={setThreads}
        useMock={useMock}
        onUseMockChange={setUseMock}
        result={scanResult}
        scanning={scanning}
      />

      <ScanProgress scanning={scanning} progress={progress} onProgress={handleProgress} />

      {drilling && (
        <div className="app-drilling">正在加载子目录…</div>
      )}

      {error && <div className="app-error">出错：{error}</div>}

      {currentNode && (
        <Breadcrumb navStack={navStack} onNavigate={handleNavigate} />
      )}

      <main className="app-main">
        <section className="panel panel-left">
          <div className="panel-head">目录明细</div>
          <DataTable rows={rows} parentSize={parentSize} onRowClick={handleDrill} />
        </section>
        <section className="panel panel-right">
          <div className="panel-head">
            空间分布（Treemap）{currentNode ? `— ${currentNode.name}` : ""}
          </div>
          <div className="panel-body">
            <TreemapChart currentNode={currentNode} onDrill={handleDrill} />
          </div>
        </section>
      </main>
    </div>
  );
}
