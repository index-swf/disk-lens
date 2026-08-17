import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ExportDialogProps {
  open: boolean;
  onClose: () => void;
}

type ExportType = "human" | "agent";

/**
 * 数据导出对话框：
 * - 导出方式：全量导出 / 过滤导出（只保留 ≥ N MB 的目录与文件，默认 500）
 * - 导出类型：
 *   - 适合人类阅读（human）：树状 YAML，保留目录层级缩进
 *   - 适合 Agent 读取（agent）：扁平 NDJSON，每行一个节点，可流式解析
 */
export default function ExportDialog({ open, onClose }: ExportDialogProps) {
  const [filter, setFilter] = useState(false);
  const [minSizeMb, setMinSizeMb] = useState(500);
  const [exportType, setExportType] = useState<ExportType>("agent");
  const [exporting, setExporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [isError, setIsError] = useState(false);

  // 每次打开时重置状态
  useEffect(() => {
    if (open) {
      setFilter(false);
      setMinSizeMb(500);
      setExportType("agent");
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
        exportType,
        filter,
        minSizeMb,
      });
      setIsError(false);
      setMessage(`已导出：\n${path}`);
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
        className="dialog dialog-export"
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
          <div className="dialog-label">导出类型</div>
          <label className="dialog-radio">
            <input
              type="radio"
              name="export-type"
              checked={exportType === "agent"}
              disabled={exporting}
              onChange={() => setExportType("agent")}
            />
            适合 Agent 读取（Agent readable）
          </label>
          <div className="dialog-format-note">
            数据格式：扁平 NDJSON（JSON Lines）—— 每行一个节点对象，含绝对路径、
            size_self / size_total / actual_size_self、depth、parent 等字段。
            层次结构：所有节点平铺，无嵌套，通过 parent 字段可重建树，便于程序流式解析。
          </div>

          <label className="dialog-radio">
            <input
              type="radio"
              name="export-type"
              checked={exportType === "human"}
              disabled={exporting}
              onChange={() => setExportType("human")}
            />
            适合人类阅读（Human readable）
          </label>
          <div className="dialog-format-note">
            数据格式：树状 YAML —— 目录层级以缩进展示，含大小、占父目录百分比等字段。
            层次结构：嵌套树，从上到下逐级展开，适合直接用文本编辑器 / 阅读器浏览。
          </div>
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
