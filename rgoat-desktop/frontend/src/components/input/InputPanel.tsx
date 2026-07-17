import { useState, useRef, useCallback, useEffect, useMemo } from "react";
import {
  Send,
  ChevronDown,
  Square,
  Paperclip,
  SlidersHorizontal,
} from "lucide-react";
import { tauriInvoke } from "../../lib/tauri-bridge";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import { useSkillStore, type SkillInfo } from "../../stores/skillStore";
import { useToastStore } from "../../stores/toastStore";

const MODES = ["Agent", "Plan", "Flow", "YOLO"] as const;
type Mode = (typeof MODES)[number];

interface InputPanelProps {
  onModeChange?: (mode: string) => void;
}

interface SendPromptResponse {
  session_id: string;
  message: string;
}

// D3-T05: 内置 Slash 命令
interface SlashItem {
  name: string; // 不含 /，如 "help"、"clear"、"skills" 或 skill 名
  description: string;
  kind: "builtin" | "skill";
}

const BUILTIN_SLASH: SlashItem[] = [
  { name: "help", description: "显示可用命令帮助", kind: "builtin" },
  { name: "clear", description: "清空当前会话消息", kind: "builtin" },
  { name: "skills", description: "列出所有可用 Skill", kind: "builtin" },
];

const HELP_TEXT = `可用 Slash 命令：
/help    — 显示本帮助
/clear   — 清空当前会话消息
/skills  — 列出所有可用 Skill
/<skill-name> — 加载指定 Skill 并发送给 Agent 执行

可用模式：Agent / Plan / Flow / YOLO`;

export default function InputPanel({ onModeChange }: InputPanelProps) {
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<Mode>("Agent");
  const [showProviderMenu, setShowProviderMenu] = useState(false);
  const [isFocused, setIsFocused] = useState(false);
  // D3-T05: Slash 命令下拉
  const [showSlashMenu, setShowSlashMenu] = useState(false);
  const [slashIndex, setSlashIndex] = useState(0);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const { providers, currentProvider, switchProvider, loadProviders } =
    useConfigStore();
  const { activeSessionId, setActiveSessionId } = useSessionStore();
  const {
    isStreaming,
    setStreaming,
    addUserMessage,
    addSystemMessage,
    clearMessages,
    cancelAgent,
    draft,
    setDraft,
    focusInputTrigger,
  } = useChatStore();
  const { skills, loadSkills, readSkill } = useSkillStore();
  const { addToast } = useToastStore();

  useEffect(() => {
    loadProviders();
    loadSkills();
  }, [loadProviders, loadSkills]);

  // P1: 监听 draft 变化（来自 Empty State 示例提问），同步到 input 并聚焦
  useEffect(() => {
    if (draft) {
      setInput(draft);
      setDraft("");
      setTimeout(() => textareaRef.current?.focus(), 0);
    }
  }, [draft, setDraft]);

  // P2: 监听全局快捷键聚焦触发器（Cmd/Ctrl+/）
  useEffect(() => {
    if (focusInputTrigger > 0) {
      setTimeout(() => textareaRef.current?.focus(), 0);
    }
  }, [focusInputTrigger]);

  const currentModel = useMemo(
    () =>
      providers.find((p) => p.is_current)?.model || currentProvider || "none",
    [providers, currentProvider]
  );

  const handleModeChange = useCallback(
    (newMode: Mode) => {
      setMode(newMode);
      onModeChange?.(newMode);
    },
    [onModeChange]
  );

  // 快捷模式切换：在 MODES 之间循环
  const cycleMode = useCallback(() => {
    const nextIndex = (MODES.indexOf(mode) + 1) % MODES.length;
    handleModeChange(MODES[nextIndex]);
  }, [mode, handleModeChange]);

  // P0-1: 取消 Agent 执行
  const handleCancel = useCallback(async () => {
    await cancelAgent();
    addToast("Agent 取消请求已发送", "info");
  }, [cancelAgent, addToast]);

  // D3-T05: 构建 Slash 命令列表（内置 + 动态 skills）
  const slashItems = useMemo<SlashItem[]>(() => {
    const skillItems: SlashItem[] = skills.map((s: SkillInfo) => ({
      name: s.name,
      description: s.description,
      kind: "skill" as const,
    }));
    return [...BUILTIN_SLASH, ...skillItems];
  }, [skills]);

  // 根据当前输入过滤 Slash 项
  const slashFiltered = useMemo<SlashItem[]>(() => {
    if (!showSlashMenu) return [];
    // input 形如 "/help" 或 "/he"
    const query = input.slice(1).toLowerCase();
    if (!query) return slashItems;
    return slashItems.filter((it) => it.name.toLowerCase().startsWith(query));
  }, [showSlashMenu, input, slashItems]);

  // 过滤列表变化时重置选中
  useEffect(() => {
    setSlashIndex(0);
  }, [slashFiltered]);

  // D3-T05: 处理 Slash 命令发送
  const handleSlashCommand = useCallback(
    async (raw: string): Promise<boolean> => {
      // 返回 true 表示已本地处理，无需发送给 Agent
      const trimmed = raw.trim();
      const cmd = trimmed.slice(1).split(/\s+/)[0]?.toLowerCase();
      const rest = trimmed.slice(1).split(/\s+/).slice(1).join(" ");

      if (cmd === "help") {
        addSystemMessage(HELP_TEXT);
        return true;
      }
      if (cmd === "clear") {
        clearMessages();
        return true;
      }
      if (cmd === "skills") {
        if (skills.length === 0) {
          addSystemMessage(
            "当前没有可用的 Skill。可在 ~/.goat/skills/<name>/SKILL.md 或 <workspace>/.goat/skills/<name>/SKILL.md 创建。"
          );
        } else {
          const list = skills
            .map((s) => `- **/${s.name}** (${s.source}) — ${s.description}`)
            .join("\n");
          addSystemMessage(`可用 Skill（${skills.length} 个）：\n${list}`);
        }
        return true;
      }
      // 检查是否匹配某个 skill
      const matched = skills.find((s) => s.name.toLowerCase() === cmd);
      if (matched) {
        const content = await readSkill(matched.name);
        if (!content) {
          addSystemMessage(`⚠️ Skill "${matched.name}" 内容读取失败。`);
          return true;
        }
        // 加载 Skill 内容作为上下文发送给 Agent
        const promptBody = rest
          ? `请按照以下 Skill 定义执行任务：

---
${content}
---

用户附加指令：${rest}`
          : `请按照以下 Skill 定义执行任务：

---
${content}
---`;

        addUserMessage(raw); // 显示用户输入的 slash 命令
        try {
          setStreaming(true);
          const res = await tauriInvoke<SendPromptResponse>("send_prompt", {
            prompt: promptBody,
            session_id: activeSessionId || undefined,
            mode: mode.toLowerCase(),
          });
          if (res?.session_id) {
            setActiveSessionId(res.session_id);
          }
        } catch (err) {
          setStreaming(false);
          const errorMsg = err instanceof Error ? err.message : String(err);
          addToast(errorMsg, "error");
        }
        return true;
      }
      return false;
    },
    [
      skills,
      readSkill,
      addSystemMessage,
      clearMessages,
      addUserMessage,
      setStreaming,
      activeSessionId,
      mode,
      setActiveSessionId,
      addToast,
    ]
  );

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || isStreaming) return;

    // D3-T05: 拦截 Slash 命令
    if (text.startsWith("/")) {
      setShowSlashMenu(false);
      setInput("");
      const handled = await handleSlashCommand(text);
      if (handled) return;
      // 未识别的 slash 命令 → 作为普通文本发送
      addUserMessage(text);
    } else {
      setInput("");
      addUserMessage(text);
    }

    try {
      setStreaming(true);
      const res = await tauriInvoke<SendPromptResponse>("send_prompt", {
        prompt: text,
        session_id: activeSessionId || undefined,
        mode: mode.toLowerCase(),
      });
      if (res?.session_id) {
        setActiveSessionId(res.session_id);
      }
    } catch (err) {
      setStreaming(false);
      const errorMsg = err instanceof Error ? err.message : String(err);
      addToast(errorMsg, "error");
    }
  }, [
    input,
    isStreaming,
    activeSessionId,
    mode,
    addUserMessage,
    setStreaming,
    setActiveSessionId,
    handleSlashCommand,
    addToast,
  ]);

  // D3-T05: 选中 Slash 项 → 插入到输入框
  const selectSlash = useCallback((item: SlashItem) => {
    setInput(`/${item.name} `);
    setShowSlashMenu(false);
    setTimeout(() => textareaRef.current?.focus(), 0);
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      // D3-T05: Slash 下拉导航
      if (showSlashMenu && slashFiltered.length > 0) {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          setSlashIndex((prev) =>
            Math.min(prev + 1, slashFiltered.length - 1)
          );
          return;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          setSlashIndex((prev) => Math.max(prev - 1, 0));
          return;
        }
        if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
          e.preventDefault();
          selectSlash(slashFiltered[slashIndex]);
          return;
        }
        if (e.key === "Escape") {
          e.preventDefault();
          setShowSlashMenu(false);
          return;
        }
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [showSlashMenu, slashFiltered, slashIndex, selectSlash, handleSend]
  );

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const v = e.target.value;
      setInput(v);
      // 检测 `/` 开头 → 显示 Slash 下拉
      if (v.startsWith("/")) {
        setShowSlashMenu(true);
      } else if (showSlashMenu) {
        setShowSlashMenu(false);
      }
    },
    [showSlashMenu]
  );

  const adjustHeight = useCallback(() => {
    const ta = textareaRef.current;
    if (ta) {
      ta.style.height = "auto";
      ta.style.height = Math.min(ta.scrollHeight, 200) + "px";
    }
  }, []);

  useEffect(() => {
    adjustHeight();
  }, [input, adjustHeight]);

  const modelButtonLabel = currentProvider
    ? currentModel && currentModel !== currentProvider
      ? `${currentProvider} / ${currentModel}`
      : currentProvider
    : "Select Provider";

  return (
    <div className="fixed bottom-[44px] left-1/2 -translate-x-1/2 w-full max-w-[840px] px-4 z-20">
      <div
        className={`flex gap-2 p-2 rounded-xl bg-surface border shadow-sm transition-colors ${
          isFocused
            ? "border-primary ring-2 ring-ring"
            : "border-border"
        }`}
      >
        {/* 左侧：附件按钮 */}
        <button
          type="button"
          onClick={() => addToast("附件功能待接入", "info")}
          className="shrink-0 p-2 rounded-lg text-text-secondary hover:bg-surface-hover transition-colors"
          aria-label="添加附件"
          title="添加附件"
        >
          <Paperclip size={18} />
        </button>

        {/* 文本域 + Slash 命令下拉 */}
        <div className="flex-1 relative">
          <textarea
            ref={textareaRef}
            className="w-full px-3 py-2.5 rounded-lg bg-bg border border-border text-text text-sm placeholder:text-text-tertiary resize-none outline-none focus:border-primary min-h-[48px] max-h-[200px]"
            placeholder={
              mode === "Plan"
                ? "在 Plan Mode 中，agent 将只读取/搜索/分析... (输入 / 查看命令)"
                : "Ask anything... (Enter to send, Shift+Enter for newline, 输入 / 查看 Slash 命令)"
            }
            value={input}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            onFocus={() => setIsFocused(true)}
            onBlur={() => setIsFocused(false)}
            disabled={isStreaming}
            rows={1}
          />

          {/* D3-T05: Slash 命令下拉 */}
          {showSlashMenu && slashFiltered.length > 0 && (
            <div className="absolute bottom-full left-0 mb-2 z-30 bg-surface border border-border rounded-lg shadow-md py-1 min-w-[320px] max-h-[280px] overflow-y-auto">
              <div className="px-3 py-1 text-[10px] text-text-secondary border-b border-border mb-1">
                Slash 命令 (↑↓ 选择, Tab/Enter 确认, Esc 关闭)
              </div>
              {slashFiltered.map((item, idx) => (
                <button
                  key={`${item.kind}-${item.name}`}
                  className={`w-full flex items-center gap-2 px-3 py-1.5 text-left transition-colors ${
                    idx === slashIndex
                      ? "bg-primary-subtle text-brand"
                      : "text-text hover:bg-surface-hover"
                  }`}
                  onMouseEnter={() => setSlashIndex(idx)}
                  onClick={() => selectSlash(item)}
                >
                  <span className="text-sm font-mono shrink-0">/{item.name}</span>
                  <span className="text-xs text-text-secondary truncate flex-1">
                    {item.description}
                  </span>
                  <span className="text-[9px] text-text-secondary shrink-0 uppercase border border-border rounded px-1">
                    {item.kind === "builtin" ? "cmd" : item.kind}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>

        {/* 右侧工具区：模型选择器 / 模式切换 / 输入选项 / 发送-取消 */}
        {/* 1. 模型选择器 */}
        <div className="relative shrink-0">
          <button
            type="button"
            onClick={() => setShowProviderMenu(!showProviderMenu)}
            className="flex items-center gap-1 px-2.5 py-2 h-full rounded-lg border border-border bg-transparent text-xs text-text-secondary hover:bg-surface-hover transition-colors whitespace-nowrap"
            aria-label="切换模型 Provider"
            title="切换模型 Provider"
          >
            <span className="truncate max-w-[140px]">{modelButtonLabel}</span>
            <ChevronDown size={12} />
          </button>

          {showProviderMenu && (
            <>
              <div
                className="fixed inset-0 z-10"
                onClick={() => setShowProviderMenu(false)}
              />
              <div className="absolute bottom-full right-0 mb-2 z-20 bg-surface border border-border rounded-lg shadow-md py-1 min-w-[180px]">
                {providers.map((p) => (
                  <button
                    key={p.name}
                    type="button"
                    onClick={() => {
                      switchProvider(p.name);
                      setShowProviderMenu(false);
                    }}
                    className={`block w-full text-left px-3 py-1.5 text-xs hover:bg-surface-hover transition-colors ${
                      p.is_current ? "text-brand" : "text-text-secondary"
                    }`}
                  >
                    <div>{p.name}</div>
                    <div className="text-[10px] text-text-secondary">{p.model}</div>
                  </button>
                ))}
                {providers.length === 0 && (
                  <div className="px-3 py-1.5 text-xs text-text-secondary">
                    No providers
                  </div>
                )}
              </div>
            </>
          )}
        </div>

        {/* 2. 快捷模式切换 */}
        <button
          type="button"
          onClick={cycleMode}
          className="shrink-0 px-2.5 py-2 rounded-lg text-xs font-medium bg-surface-active text-brand hover:bg-surface-hover transition-colors"
          aria-label={`当前模式 ${mode}，点击切换`}
          title={`当前模式 ${mode}，点击切换`}
        >
          {mode}
        </button>

        {/* 3. 输入选项 */}
        <button
          type="button"
          onClick={() => addToast("输入选项待展开", "info")}
          className="shrink-0 p-2 rounded-lg text-text-secondary hover:bg-surface-hover transition-colors"
          aria-label="输入选项"
          title="输入选项"
        >
          <SlidersHorizontal size={18} />
        </button>

        {/* 4. 发送 / 取消 */}
        {isStreaming ? (
          <button
            type="button"
            onClick={handleCancel}
            title="取消 Agent 执行"
            aria-label="取消 Agent 执行"
            className="px-4 py-2 rounded-lg bg-error text-white font-semibold text-sm hover:bg-error/90 transition-colors shrink-0 self-end flex items-center justify-center"
          >
            <Square size={14} className="fill-white" />
          </button>
        ) : (
          <button
            type="button"
            onClick={handleSend}
            disabled={!input.trim()}
            className="px-4 py-2 rounded-lg bg-primary text-white font-semibold text-sm hover:bg-primary-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors shrink-0 self-end"
            aria-label="发送"
            title="发送"
          >
            <Send size={16} />
          </button>
        )}
      </div>
    </div>
  );
}
