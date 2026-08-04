import { useState } from "react";
import type { ScanResult } from "../types";
import { formatBytes } from "../utils";

interface StatusBarProps {
  result: ScanResult | null;
}

/**
 * 底部状态条：可用空间/总大小、总文件数、文件系统、簇大小，
 * 以及扫描错误状态（✓ 全部正常 / ✗ N 个错误，点击弹窗查看明细）。
 */
export default function StatusBar({ result }: StatusBarProps) {
  const [showErrors, setShowErrors] = useState(false);
  if (!result) return null;

  const { errors } = result;
  const ok = errors.count === 0;

  return (
    <>
      <footer className="status-bar">
        <button
          type="button"
          className={
            "status-err-btn " + (ok ? "status-err-ok" : "status-err-bad")
          }
          onClick={() => setShowErrors(true)}
          disabled={ok}
          title={ok ? "扫描无错误" : "点击查看错误日志"}
        >
          {ok ? "✓ 全部正常" : `✗ ${errors.count.toLocaleString()} 个错误`}
        </button>
        <span className="status-item">
          可用空间 <b>{formatBytes(result.free_bytes)}</b>{" "}
          <span className="status-dim">/</span> 总大小{" "}
          <b>{formatBytes(result.total_bytes)}</b>
        </span>
        <span className="status-item">
          文件总数 <b>{result.total_files.toLocaleString()}</b>
        </span>
        <span className="status-item">
          文件系统 <b>{result.fs_type || "—"}</b>
        </span>
        <span className="status-item">
          簇大小{" "}
          <b>{result.cluster_size > 0 ? formatBytes(result.cluster_size) : "—"}</b>
        </span>
      </footer>

      {showErrors && (
        <div className="modal-backdrop" onClick={() => setShowErrors(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="modal-title">
                扫描错误日志（共 {errors.count.toLocaleString()} 条）
              </span>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowErrors(false)}
              >
                ✕
              </button>
            </div>
            <div className="modal-body">
              {errors.samples.length === 0 ? (
                <p className="modal-empty">
                  {errors.count > 0
                    ? "错误数超过日志上限，明细未全部记录。"
                    : "无错误明细。"}
                </p>
              ) : (
                <ul className="modal-list">
                  {errors.samples.map((s, i) => (
                    <li key={i} className="modal-item">
                      {s}
                    </li>
                  ))}
                  {errors.count > errors.samples.length && (
                    <li className="modal-item modal-more">
                      ……另有 {errors.count - errors.samples.length} 条未展示
                    </li>
                  )}
                </ul>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
