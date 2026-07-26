import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ScanProgress as ScanProgressType } from "../types";

interface ScanProgressProps {
  /** 是否正在扫描（决定是否展示进度条并接收事件） */
  scanning: boolean;
  /** 最新一次进度的快照（由父级持有并传入，用于展示） */
  progress: ScanProgressType | null;
  /** 扫描进度变化回调，向上抛出给 App 维护状态 */
  onProgress: (progress: ScanProgressType) => void;
}

// 该组件负责真实监听后端 emit 的 "scan-progress" 事件。
// 监听器只在挂载时注册一次，通过 ref 判断是否处于扫描态，避免重复订阅。
export default function ScanProgress({ scanning, progress, onProgress }: ScanProgressProps) {
  const scanningRef = useRef(scanning);
  const onProgressRef = useRef(onProgress);

  scanningRef.current = scanning;
  onProgressRef.current = onProgress;

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    (async () => {
      const fn = await listen<ScanProgressType>("scan-progress", (event) => {
        if (scanningRef.current && onProgressRef.current) {
          onProgressRef.current(event.payload);
        }
      });
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!scanning) return null;

  return (
    <div className="scan-progress" role="status" aria-live="polite">
      <div className="scan-progress-spinner" />
      <div className="scan-progress-text">
        <span className="scan-progress-title">正在扫描…</span>
        {progress && (
          <span className="scan-progress-stats">
            已扫描文件 {progress.scanned_files.toLocaleString()} · 文件夹{" "}
            {progress.scanned_folders.toLocaleString()}
          </span>
        )}
        {progress?.current_dir && (
          <span className="scan-progress-current">当前目录：{progress.current_dir}</span>
        )}
      </div>
    </div>
  );
}
