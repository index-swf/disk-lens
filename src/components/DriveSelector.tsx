import { useState } from "react";

interface DriveSelectorProps {
  value: string;
  onChange: (drivePath: string) => void;
  disabled?: boolean;
}

// 先用常见 Windows 盘符简单实现；后续可改为调用后端枚举真实卷。
const COMMON_DRIVES = ["C:", "D:", "E:", "F:", "G:"];

export default function DriveSelector({ value, onChange, disabled }: DriveSelectorProps) {
  const [custom, setCustom] = useState("");

  return (
    <div className="drive-selector">
      <label htmlFor="drive-select">驱动器：</label>
      <select
        id="drive-select"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
      >
        {COMMON_DRIVES.map((d) => (
          <option key={d} value={d}>
            {d}
          </option>
        ))}
      </select>
      <input
        type="text"
        placeholder="自定义路径 (如 \\\\.\\C:)"
        value={custom}
        disabled={disabled}
        onChange={(e) => setCustom(e.target.value)}
        onBlur={() => {
          if (custom.trim()) onChange(custom.trim());
        }}
        className="drive-custom-input"
      />
    </div>
  );
}
