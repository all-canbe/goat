// D1-T05: 批准范围选择器 — 4 档 radio 按钮 + 快捷键提示

interface ApprovalScopeSelectorProps {
  allowOptions: string[];
  value: string;
  onChange: (scope: string) => void;
  disabled?: boolean;
}

const SCOPE_OPTIONS: Array<{
  value: string;
  label: string;
  hint: string;
  shortcut: string;
}> = [
  { value: "once", label: "Once", hint: "仅本次", shortcut: "Y" },
  { value: "session", label: "Session", hint: "本会话内同工具自动放行", shortcut: "A" },
  { value: "all_similar", label: "All Similar", hint: "同工具同参数才放行", shortcut: "S" },
  { value: "always", label: "Always", hint: "永久允许（暂按 Session 处理）", shortcut: "W" },
];

export default function ApprovalScopeSelector({
  allowOptions,
  value,
  onChange,
  disabled = false,
}: ApprovalScopeSelectorProps) {
  const options = SCOPE_OPTIONS.filter((o) => allowOptions.includes(o.value));

  return (
    <div className="flex flex-wrap items-center gap-1">
      {options.map((opt) => {
        const active = value === opt.value;
        return (
          <button
            key={opt.value}
            type="button"
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            title={opt.hint}
            className={`px-2 py-1 rounded text-[11px] font-medium border transition-colors ${
              active
                ? "bg-primary text-white border-primary"
                : "bg-surface text-text-secondary border-border hover:text-text hover:border-text-secondary"
            } disabled:opacity-50 disabled:cursor-not-allowed`}
          >
            <span className="mr-1 opacity-70">[{opt.shortcut}]</span>
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
