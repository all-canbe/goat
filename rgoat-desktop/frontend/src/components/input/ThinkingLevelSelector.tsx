import { useState, useRef, useEffect, useCallback } from "react";
import { ChevronDown, Check } from "lucide-react";

export type ThinkingLevel = "default" | "low" | "medium" | "high" | "max";

interface ThinkingLevelOption {
  value: ThinkingLevel;
  label: string;
  hint: string;
}

const OPTIONS: ThinkingLevelOption[] = [
  { value: "default", label: "Default", hint: "使用模型默认" },
  { value: "low", label: "Low", hint: "reasoning_effort: low" },
  { value: "medium", label: "Medium", hint: "reasoning_effort: medium" },
  { value: "high", label: "High", hint: "reasoning_effort: high" },
  { value: "max", label: "Max", hint: "reasoning_effort: high" },
];

interface ThinkingLevelSelectorProps {
  value: ThinkingLevel;
  onChange: (v: ThinkingLevel) => void;
  disabled?: boolean;
}

export default function ThinkingLevelSelector({
  value,
  onChange,
  disabled,
}: ThinkingLevelSelectorProps) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  const current = OPTIONS.find((o) => o.value === value) ?? OPTIONS[0];

  const handleSelect = useCallback(
    (v: ThinkingLevel) => {
      onChange(v);
      setOpen(false);
    },
    [onChange]
  );

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    window.addEventListener("mousedown", handleClick);
    return () => window.removeEventListener("mousedown", handleClick);
  }, [open]);

  // Esc 关闭
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
      }
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open]);

  return (
    <div ref={wrapRef} className="relative shrink-0">
      <button
        type="button"
        onClick={() => !disabled && setOpen((v) => !v)}
        disabled={disabled}
        aria-label="思考强度"
        aria-haspopup="listbox"
        aria-expanded={open}
        title={`思考强度: ${current.label}`}
        className="flex items-center gap-1 h-7 px-2 rounded-lg text-xs text-text-secondary hover:text-text hover:bg-surface-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed whitespace-nowrap"
      >
        <span>{current.label}</span>
        <ChevronDown size={12} />
      </button>
      {open && (
        <div
          role="listbox"
          aria-label="思考强度选项"
          className="absolute bottom-full left-0 mb-2 z-30 bg-surface border border-border rounded-lg shadow-md py-1 min-w-[200px]"
        >
          {OPTIONS.map((o) => (
            <button
              key={o.value}
              role="option"
              aria-selected={o.value === value}
              onClick={() => handleSelect(o.value)}
              className={`w-full flex items-center gap-2 px-3 py-1.5 text-left transition-colors ${
                o.value === value
                  ? "bg-primary-subtle text-brand"
                  : "text-text hover:bg-surface-hover"
              }`}
            >
              <div className="flex-1 min-w-0">
                <div className="text-sm">{o.label}</div>
                <div className="text-[10px] text-text-secondary truncate">
                  {o.hint}
                </div>
              </div>
              {o.value === value && <Check size={12} className="shrink-0" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
