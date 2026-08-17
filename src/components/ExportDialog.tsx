import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ExportDialogProps {
  open: boolean;
  onClose: () => void;
}

/**
 * 数据导出对话框（简单版）：
 * - 导出方式：全量导出 / 过滤导出（只保留 ≥ N MB 的目录与文件）
 * - 阈值：仅过滤模式显示，默认 500 MB
 * - 格式：JSON（固定）
 * 确认后由后端弹出系统「另存为」对话框，返回实际保存路径。
 */
export default function ExportDialog({ open, onClose }: ExportDialogProps) {
  const [filter, setFilter] = useState(false);
  const [minSizeMb, setMinSizeMb] = useState(500);
  const [exporting, setExporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [isError, setIsError] = useState(false);

  // 每次打开时重置状态
  useEffect(() => {
    if (open) {
      setFilter(false);
      setMinSizeMb(500);
      setExporting(false);
      setMessage(null);
      setIsError(false);
    }
  }, [open]);

  if (!open) return null;

  const doExport = async () => {
    setExporting(true);
    setMessage(null);
    try {
      const path = await invoke<string>("export_scan_data", {
        filter,
        minSizeMb,
      });
      setIsError(false);
      setMessage(`已导出到：${path}`);
    } catch (e) {
      setIsError(true);
      setMessage(typeof e === "string" ? e : JSON.stringify(e));
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="dialog-overlay" onClick={exporting ? undefined : onClose}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="dialog-title">导出扫描数据</h2>

        <div className="dialog-section">
          <div className="dialog-label">导出方式</div>
          <label className="dialog-radio">
            <input
              type="radio"
              name="export-mode"
              checked={!filter}
              disabled={exporting}
              onChange={() => setFilter(false)}
            />
            全量导出
          </label>
          <label className="dialog-radio">
            <input
              type="radio"
              name="export-mode"
              checked={filter}
              disabled={exporting}
              onChange={() => setFilter(true)}
            />
            过滤导出（只保留 ≥ N MB 的目录 / 文件）
          </label>
        </div>

        {filter && (
          <div className="dialog-section">
            <label className="dialog-label" htmlFor="export-min-mb">
              阈值（MB）
            </label>
            <input
              id="export-min-mb"
              type="number"
              min={1}
              max={1000000}
              value={minSizeMb}
              disabled={exporting}
              onChange={(e) =>
                setMinSizeMb(Math.max(1, Number(e.target.value) || 1))
              }
              className="dialog-number"
            />
          </div>
        )}

        <div className="dialog-section">
          <div className="dialog-label">文件格式</div>
          <span className="dialog-format-note">
            JSON（含每个节点的绝对路径，适合工具 / AI agent 读取）
          </span>
        </div>

        {message && (
          <div className={isError ? "dialog-msg dialog-msg-error" : "dialog-msg"}>
            {message}
          </div>
        )}

        <div className="dialog-actions">
          <button
            type="button"
            className="dialog-btn"
            onClick={onClose}
            disabled={exporting}
          >
            关闭
          </button>
          <button
            type="button"
            className="dialog-btn dialog-btn-primary"
            onClick={doExport}
            disabled={exporting}
          >
            {exporting ? "导出中…" : "导出"}
          </button>
        </div>
      </div>
    </div>
  );
}
