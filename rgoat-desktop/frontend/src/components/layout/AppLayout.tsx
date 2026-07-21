import { useState, useCallback, useEffect } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import Sidebar from "./Sidebar";
import ChatArea from "../chat/ChatArea";
import StatusBar from "../status/StatusBar";
import TitleBar from "./TitleBar";
import RightPanel from "./RightPanel";
import ApprovalDialog from "../dialogs/ApprovalDialog";
import PlanPreviewDialog from "../dialogs/PlanPreviewDialog";
import SettingsDialog from "../dialogs/SettingsDialog";
import AddProviderDialog from "../dialogs/AddProviderDialog";
import ModelManagerDialog from "../dialogs/ModelManagerDialog";
import CommandPalette from "../command/CommandPalette";
import Toaster from "../feedback/Toaster";
import { useAgentEvents } from "../../hooks/useAgentEvents";
import { useGlobalShortcuts } from "../../hooks/useGlobalShortcuts";
import { useConfigStore } from "../../stores/configStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useChatStore } from "../../stores/chatStore";
import { useChangesStore } from "../../stores/changesStore";
import { useThemeStore } from "../../stores/themeStore";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { useToastStore } from "../../stores/toastStore";

// P1: Sidebar 折叠状态 localStorage key
const SIDEBAR_COLLAPSED_KEY = "sidebar_collapsed";

export default function AppLayout() {
  const { isStreaming } = useAgentEvents();
  const { loadProviders } = useConfigStore();
  const [currentMode, setCurrentMode] = useState("Agent");
  const [isCommandPaletteOpen, setCommandPaletteOpen] = useState(false);
  // P2: 设置页 Modal 状态
  const [isSettingsOpen, setSettingsOpen] = useState(false);
  // Provider 管理弹窗状态（根层渲染，避免被 Composer transform 容器裁切）
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [showManageProviders, setShowManageProviders] = useState(false);
  // P1: 从 localStorage 初始化折叠状态
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
    } catch {
      return false;
    }
  });
  // 右侧面板可见性
  const [rightPanelVisible, setRightPanelVisible] = useState(true);

  const { createSession, loadSessions } = useSessionStore();
  const { clearMessages, triggerFocusInput, planContent, clearPlanContent } = useChatStore();
  const { loadWorkspace, setWorkspace } = useWorkspaceStore();
  const addToast = useToastStore((s) => s.addToast);

  // D1-T07: 从 store 读取统计计数
  const pendingApprovals = useChatStore((s) => s.pendingApprovals);
  const toolCallCount = useChatStore((s) => s.toolCallCount);
  const tokenUsage = useChatStore((s) => s.tokenUsage);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const changesBySession = useChangesStore((s) => s.changesBySession);
  const totalChanges = activeSessionId
    ? (changesBySession[activeSessionId]?.length || 0)
    : 0;

  const handleModeChange = useCallback((mode: string) => {
    setCurrentMode(mode);
  }, []);

  // P1: 切换 Sidebar 折叠状态并持久化
  const toggleSidebar = useCallback(() => {
    setSidebarCollapsed((prev) => {
      const next = !prev;
      try {
        localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(next));
      } catch {
        // localStorage 不可用时静默忽略
      }
      return next;
    });
  }, []);

  // 切换右侧面板可见性
  const toggleRightPanel = useCallback(() => {
    setRightPanelVisible((prev) => !prev);
  }, []);

  const handleNewSession = useCallback(async () => {
    clearMessages();
    try {
      await createSession();
    } catch {
      // createSession 失败时不阻断 UI
    }
  }, [clearMessages, createSession]);

  const handleClearMessages = useCallback(() => {
    clearMessages();
  }, [clearMessages]);

  // 选择目录并切换 workspace；成功后刷新 FileTree（通过 workspace path 变化触发 key 重挂载）
  // 并创建归属新目录的会话；agent 运行时后端返回错误，显示错误 toast
  const handleAddWorkspace = useCallback(async () => {
    let selected: string | null = null;
    try {
      const result = await openDialog({ directory: true, multiple: false });
      selected = typeof result === "string" ? result : null;
    } catch {
      // 对话框取消或不可用，静默忽略
      return;
    }
    if (!selected) return;

    try {
      await setWorkspace(selected);
      addToast("工作空间已切换", "success");
      // 切换 workspace 后创建归属新目录的会话
      clearMessages();
      try {
        await createSession();
      } catch {
        // createSession 失败不影响 workspace 切换结果
      }
    } catch (err) {
      const msg =
        err instanceof Error ? err.message : "切换工作空间失败";
      addToast(msg, "error");
    }
  }, [setWorkspace, addToast, clearMessages, createSession]);

  // P2: 全局快捷键关闭对话框（优先级：CommandPalette > SettingsDialog > PlanPreviewDialog）
  // ApprovalDialog 已自行处理 Esc；PlanPreviewDialog 也自行处理 Esc，此处仅作为兜底
  const handleCloseDialog = useCallback(() => {
    if (isCommandPaletteOpen) {
      setCommandPaletteOpen(false);
      return;
    }
    if (isSettingsOpen) {
      setSettingsOpen(false);
      return;
    }
    if (planContent) {
      clearPlanContent();
      return;
    }
  }, [isCommandPaletteOpen, isSettingsOpen, planContent, clearPlanContent]);

  // P2: 主题初始化（读取 localStorage + 注册系统主题变化监听）
  const initTheme = useThemeStore((s) => s.init);
  useEffect(() => {
    return initTheme();
  }, [initTheme]);

  // P2: 集中管理全局快捷键
  useGlobalShortcuts({
    onToggleCommandPalette: useCallback(() => {
      setCommandPaletteOpen((prev) => !prev);
    }, []),
    onNewSession: handleNewSession,
    onToggleSidebar: toggleSidebar,
    onToggleRightPanel: toggleRightPanel,
    onFocusInput: triggerFocusInput,
    onCloseDialog: handleCloseDialog,
  });

  useEffect(() => {
    loadProviders();
    // 启动时加载 workspace 信息
    loadWorkspace().catch(() => {});
    // 启动时加载会话列表；若为空则创建一个新会话
    loadSessions().then(() => {
      if (useSessionStore.getState().sessions.length === 0) {
        createSession().catch(() => {});
      }
    });
  }, [loadProviders, loadWorkspace, loadSessions, createSession]);

  return (
    <div className="flex flex-col h-screen bg-bg text-text">
      <TitleBar
        onToggleSidebar={toggleSidebar}
        onToggleCommandPalette={() => setCommandPaletteOpen((prev) => !prev)}
        onToggleRightPanel={toggleRightPanel}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <div className="flex flex-1 overflow-hidden">
        <Sidebar collapsed={sidebarCollapsed} />

        <div className="flex flex-col flex-1 overflow-hidden">
          {currentMode === "Plan" && (
            <div className="px-4 py-1.5 bg-warning-subtle border-b border-warning/30 text-warning text-xs text-center">
              Plan Mode：写工具被禁用，agent 仅可读取/搜索/分析。切换到 Agent Mode 后可执行修改。
            </div>
          )}
          <ChatArea
            onModeChange={handleModeChange}
            onAddProvider={() => setShowAddProvider(true)}
            onManageProviders={() => setShowManageProviders(true)}
          />
        </div>

        {rightPanelVisible && <RightPanel onAddWorkspace={handleAddWorkspace} />}
      </div>

      {/* Status bar */}
      <StatusBar
        isStreaming={isStreaming}
        currentMode={currentMode}
        pendingApprovals={pendingApprovals}
        totalChanges={totalChanges}
        toolCallCount={toolCallCount}
        tokenUsage={tokenUsage}
      />

      {/* Approval dialog */}
      <ApprovalDialog />

      {/* P1: Plan Mode 计划文档预览 Modal */}
      <PlanPreviewDialog onAccept={() => handleModeChange("Agent")} />

      {/* P0-4: 全局 Toast 通知 */}
      <Toaster />

      {/* Command palette */}
      <CommandPalette
        isOpen={isCommandPaletteOpen}
        onClose={() => setCommandPaletteOpen(false)}
        onModeChange={handleModeChange}
        onNewSession={handleNewSession}
        onClearMessages={handleClearMessages}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      {/* P2: 设置页 Modal */}
      <SettingsDialog
        isOpen={isSettingsOpen}
        onClose={() => setSettingsOpen(false)}
        defaultMode={currentMode}
        onDefaultModeChange={handleModeChange}
      />

      {/* Provider 管理弹窗（根层渲染，避免被 Composer transform 容器裁切） */}
      <AddProviderDialog
        isOpen={showAddProvider}
        onClose={() => setShowAddProvider(false)}
      />
      <ModelManagerDialog
        isOpen={showManageProviders}
        onClose={() => setShowManageProviders(false)}
        onAddProvider={() => {
          setShowManageProviders(false);
          setShowAddProvider(true);
        }}
      />
    </div>
  );
}
