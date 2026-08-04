import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DriveInfo } from "../types";

interface DriveSelectorProps {
  value: string;
  onChange: (drivePath: string) => void;
  disabled?: boolean;
}

const RECENT_KEY = "tree-scan.recent-scans";
const MAX_RECENT = 5;

/** 读取本地保存的最近扫描路径（新的在前，最多 5 条） */
export function loadRecents(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? arr.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** 记录一次扫描路径：去重、插到最前、截断为 5 条，并写回 localStorage */
export function pushRecent(path: string): string[] {
  if (!path) return loadRecents();
  const next = [path, ...loadRecents().filter((p) => p !== path)].slice(0, MAX_RECENT);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    /* localStorage 不可用时静默忽略 */
  }
  return next;
}

/**
 * 扫描目标选择器：点击弹出下拉菜单，含三部分——
 * 1) 本机真实存在的磁盘（后端 list_drives 枚举，不含不存在的盘符）；
 * 2) 最近 5 次扫描路径（localStorage，快速复用）；
 * 3) 选择自定义扫描路径（调用系统原生文件夹选择对话框）。
 */
export default function DriveSelector({ value, onChange, disabled }: DriveSelectorProps) {
  const [open, setOpen] = useState(false);
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [drivesError, setDrivesError] = useState<string | null>(null);
  const [recents, setRecents] = useState<string[]>(loadRecents());
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // 每次打开菜单时刷新磁盘列表与最近记录。
  useEffect(() => {
    if (!open) return;
    setRecents(loadRecents());
    setDrivesError(null);
    invoke<DriveInfo[]>("list_drives")
      .then((d) => {
        setDrives(d);
        setDrivesError(null);
      })
      .catch((e) => {
        setDrives([]);
        setDrivesError(typeof e === "string" ? e : JSON.stringify(e));
      });
  }, [open]);

  // 点击菜单外部时关闭。
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const select = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  const pickCustom = async () => {
    setPicking(true);
    setPickError(null);
    try {
      const p = await invoke<string | null>("pick_folder");
      if (p) select(p);
    } catch (e) {
      setPickError(typeof e === "string" ? e : JSON.stringify(e));
    } finally {
      setPicking(false);
    }
  };

  return (
    <div className="drive-selector" ref={rootRef}>
      <button
        type="button"
        className="drive-btn"
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        title="选择扫描目标"
      >
        <span className="drive-btn-icon">📁</span>
        <span className="drive-btn-label">{value}</span>
        <span className="drive-btn-caret">{open ? "▲" : "▼"}</span>
      </button>

      {open && (
        <div className="drive-menu">
          <div className="drive-menu-section">
            <div className="drive-menu-title">磁盘</div>
            {drivesError ? (
              <div className="drive-menu-empty drive-menu-error">
                读取失败：{drivesError}
              </div>
            ) : drives.length === 0 ? (
              <div className="drive-menu-empty">正在读取…</div>
            ) : (
              drives.map((d) => (
                <button
                  key={d.letter}
                  type="button"
                  className="drive-menu-item drive-item"
                  onClick={() => select(d.letter)}
                  title={`${d.letter} · ${d.kind}${d.label ? ` · ${d.label}` : ""}`}
                >
                  <span className="drive-item-letter">{d.letter}</span>
                  <span className="drive-item-kind">{d.kind}</span>
                  {d.label && <span className="drive-item-label">{d.label}</span>}
                </button>
              ))
            )}
          </div>

          <div className="drive-menu-section">
            <div className="drive-menu-title">最近扫描</div>
            {recents.length === 0 ? (
              <div className="drive-menu-empty">暂无记录</div>
            ) : (
              recents.map((r) => (
                <button
                  key={r}
                  type="button"
                  className="drive-menu-item"
                  onClick={() => select(r)}
                >
                  {r}
                </button>
              ))
            )}
          </div>

          <div className="drive-menu-section">
            <button
              type="button"
              className="drive-menu-item drive-menu-custom"
              onClick={pickCustom}
              disabled={picking}
            >
              {picking ? "请选择文件夹…" : "选择自定义扫描路径…"}
            </button>
            {pickError && (
              <div className="drive-menu-empty drive-menu-error">
                打开对话框失败：{pickError}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
