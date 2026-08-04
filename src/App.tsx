import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  ScanMethod,
  ScanProgress as ScanProgressType,
  ScanResult,
  TreeNode,
} from "./types";
import DriveSelector, { pushRecent } from "./components/DriveSelector";
import ScanProgress from "./components/ScanProgress";
import DebugPanel from "./components/DebugPanel";
import StatusBar from "./components/StatusBar";
import TreeTable, { type SortKey, type SortDir } from "./components/TreeTable";
import "./App.css";

/** 按 path key（"C:/Users/index"）在 root（+loaded 覆盖）中定位节点 */
function findNode(
  root: TreeNode,
  loaded: Map<string, TreeNode>,
  key: string
): TreeNode | null {
  const segs = key.split("/");
  let cur: TreeNode = loaded.get(segs[0]) ?? root;
  for (let i = 1; i < segs.length; i++) {
    const seg = segs[i];
    const child: TreeNode | undefined =
      cur.children.find((c) => c.name === seg) ??
      cur.children.find((c) => c.name.toLowerCase() === seg.toLowerCase());
    if (!child) return null;
    const childKey = segs.slice(0, i + 1).join("/");
    cur = loaded.get(childKey) ?? child;
  }
  return cur;
}

export default function App() {
  const [drivePath, setDrivePath] = useState("C:");
  const [method, setMethod] = useState<ScanMethod>("auto");
  // 树裁剪参数已从 UI 移除（普通用户无需理解），固定为默认值：
  // 初始树向下展开 4 层、每层保留 Top 100、按数量截断（超出部分折叠为
  // "(其他 N 项)" 并支持展开时懒加载）。性能调优由开发侧 CLI 完成。
  const [precise, setPrecise] = useState(false);
  const [threads, setThreads] = useState(0);

  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [root, setRoot] = useState<TreeNode | null>(null);
  const [rootSize, setRootSize] = useState(0);

  // 树形展开状态：目录展开集合、[files] 虚拟节点展开集合、懒加载子树缓存。
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filesExpanded, setFilesExpanded] = useState<Set<string>>(new Set());
  const [loaded, setLoaded] = useState<Map<string, TreeNode>>(new Map());

  const [scanning, setScanning] = useState(false);
  const [drilling, setDrilling] = useState(false);
  const [progress, setProgress] = useState<ScanProgressType | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [sortKey, setSortKey] = useState<SortKey>("size");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const handleScan = useCallback(async () => {
    setScanning(true);
    setError(null);
    setScanResult(null);
    setRoot(null);
    setRootSize(0);
    setProgress(null);
    // 重置展开/懒加载状态
    setExpanded(new Set());
    setFilesExpanded(new Set());
    setLoaded(new Map());

    try {
      const result = await invoke<ScanResult>("scan_drive", {
        drivePath,
        method,
        maxDepth: 4,
        topN: 100,
        topNMode: "count",
        // 树形列表用 [files] 虚拟节点统一表达文件，因此强制不合并文件。
        mergeFiles: false,
        precise,
        threads: threads > 0 ? threads : null,
      });
      setScanResult(result);
      setRoot(result.root);
      setRootSize(result.root.size);
      // 默认展开根节点，便于直接查看。
      setExpanded(new Set([result.root.name]));
      // 记录最近扫描路径（去重、最多 5 条，本地持久化）。
      pushRecent(drivePath);
    } catch (e) {
      setError(typeof e === "string" ? e : JSON.stringify(e));
    } finally {
      setScanning(false);
    }
  }, [drivePath, method, precise, threads]);

  const onToggleDir = useCallback(
    async (key: string) => {
      const wasExpanded = expanded.has(key);
      setExpanded((prev) => {
        const next = new Set(prev);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        return next;
      });

      // 展开一个被后端裁剪（truncated）的目录时，懒加载其子树。
      if (!wasExpanded) {
        const node = findNode(root as TreeNode, loaded, key);
        if (node && node.truncated && !loaded.has(key)) {
          setDrilling(true);
          try {
            const path = key.split("/"); // ["C:","Users",...]；后端会剥掉首段根名
            const subtree = await invoke<TreeNode>("get_node", {
              path,
              maxDepth: 4,
              topN: 100,
              topNMode: "count",
              mergeFiles: false,
            });
            setLoaded((prev) => {
              const next = new Map(prev);
              next.set(key, subtree);
              return next;
            });
            if (import.meta.env.DEV) {
              console.log("[get_node] loaded", key, "children=", subtree.children.length);
            }
          } catch (e) {
            setError(typeof e === "string" ? e : JSON.stringify(e));
          } finally {
            setDrilling(false);
          }
        }
      }
    },
    [expanded, loaded, root]
  );

  const onToggleFiles = useCallback((key: string) => {
    setFilesExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const onSort = useCallback((key: SortKey) => {
    if (key === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir(key === "name" ? "asc" : "desc");
    }
  }, [sortKey]);

  const handleProgress = useCallback((p: ScanProgressType) => {
    setProgress(p);
  }, []);

  return (
    <div className="app">
      <header className="app-header">
        <h1 className="app-title">DiskLens</h1>
        <DriveSelector value={drivePath} onChange={setDrivePath} disabled={scanning} />
        <button className="scan-btn" onClick={handleScan} disabled={scanning}>
          {scanning ? "扫描中…" : "开始扫描"}
        </button>
      </header>

      {/* 调试面板仅在开发环境显示，release 打包自动隐藏 */}
      {import.meta.env.DEV && (
        <DebugPanel
          method={method}
          onMethodChange={setMethod}
          precise={precise}
          onPreciseChange={setPrecise}
          threads={threads}
          onThreadsChange={setThreads}
          result={scanResult}
          scanning={scanning}
        />
      )}

      <ScanProgress scanning={scanning} progress={progress} onProgress={handleProgress} />

      {drilling && <div className="app-drilling">正在加载子目录…</div>}
      {error && <div className="app-error">出错：{error}</div>}

      {root ? (
        <main className="app-main-single">
          <TreeTable
            root={root}
            rootSize={rootSize}
            expanded={expanded}
            filesExpanded={filesExpanded}
            loaded={loaded}
            sortKey={sortKey}
            sortDir={sortDir}
            onToggleDir={onToggleDir}
            onToggleFiles={onToggleFiles}
            onSort={onSort}
          />
        </main>
      ) : (
        <div className="app-empty">选择一个驱动器并点击「开始扫描」</div>
      )}

      <StatusBar result={scanResult} />
    </div>
  );
}
