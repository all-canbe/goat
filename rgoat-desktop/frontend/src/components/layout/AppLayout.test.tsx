import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

// --- Store mocks ---
const mockConfigStore = vi.hoisted(() => ({
  providers: [] as {
    name: string;
    model: string;
    provider_type: string;
    is_current: boolean;
    enabled: boolean;
    source: "settings" | "env" | "fallback";
  }[],
  currentProvider: "",
  hasConfiguredProvider: false,
  loading: false,
  loadProviders: vi.fn(),
}));

const mockSessionStore = vi.hoisted(() => ({
  createSession: vi.fn().mockResolvedValue(undefined),
  loadSessions: vi.fn().mockResolvedValue(undefined),
  sessions: [] as {
    id: string;
    title: string;
    message_count: number;
    created_at: string;
  }[],
  activeSessionId: "",
}));

const mockChatStore = vi.hoisted(() => ({
  clearMessages: vi.fn(),
  triggerFocusInput: vi.fn(),
  planContent: null as string | null,
  clearPlanContent: vi.fn(),
  pendingApprovals: [] as unknown[],
  toolCallCount: 0,
  tokenUsage: { input: 0, output: 0 },
}));

const mockChangesStore = vi.hoisted(() => ({
  changesBySession: {} as Record<string, unknown[]>,
}));

const mockThemeStore = vi.hoisted(() => ({
  init: vi.fn(() => undefined),
}));

const mockWorkspaceStore = vi.hoisted(() => ({
  loadWorkspace: vi.fn().mockResolvedValue(undefined),
  setWorkspace: vi.fn().mockResolvedValue(undefined),
  setTemporaryWorkspace: vi.fn().mockResolvedValue(undefined),
  workspace: null as { path: string; is_temporary: boolean } | null,
}));

const mockToastStore = vi.hoisted(() => ({
  addToast: vi.fn(),
}));

vi.mock("../../stores/configStore", () => ({
  useConfigStore: (selector?: (s: typeof mockConfigStore) => unknown) =>
    selector ? selector(mockConfigStore) : mockConfigStore,
}));

vi.mock("../../stores/sessionStore", () => ({
  useSessionStore: Object.assign(
    (selector?: (s: typeof mockSessionStore) => unknown) =>
      selector ? selector(mockSessionStore) : mockSessionStore,
    { getState: () => mockSessionStore }
  ),
}));

vi.mock("../../stores/chatStore", () => ({
  useChatStore: (selector?: (s: typeof mockChatStore) => unknown) =>
    selector ? selector(mockChatStore) : mockChatStore,
  useActiveSessionState: () => ({
    planContent: mockChatStore.planContent,
    pendingApprovals: mockChatStore.pendingApprovals,
    toolCallCount: mockChatStore.toolCallCount,
    tokenUsage: mockChatStore.tokenUsage,
  }),
}));

vi.mock("../../stores/changesStore", () => ({
  useChangesStore: (selector?: (s: typeof mockChangesStore) => unknown) =>
    selector ? selector(mockChangesStore) : mockChangesStore,
}));

vi.mock("../../stores/themeStore", () => ({
  useThemeStore: (selector?: (s: typeof mockThemeStore) => unknown) =>
    selector ? selector(mockThemeStore) : mockThemeStore,
}));

vi.mock("../../stores/workspaceStore", () => ({
  useWorkspaceStore: (selector?: (s: typeof mockWorkspaceStore) => unknown) =>
    selector ? selector(mockWorkspaceStore) : mockWorkspaceStore,
}));

vi.mock("../../stores/toastStore", () => ({
  useToastStore: (selector?: (s: typeof mockToastStore) => unknown) =>
    selector ? selector(mockToastStore) : mockToastStore,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));

// --- Hook mocks ---
vi.mock("../../hooks/useAgentEvents", () => ({
  useAgentEvents: () => ({ isStreaming: false }),
}));

vi.mock("../../hooks/useGlobalShortcuts", () => ({
  useGlobalShortcuts: () => {},
}));

// --- Component stubs (heavyweight siblings we don't need to test here) ---
vi.mock("./Sidebar", () => ({ default: () => null }));
vi.mock("../status/StatusBar", () => ({ default: () => null }));
vi.mock("./TitleBar", () => ({ default: () => null }));
vi.mock("./RightPanel", () => ({ default: () => null }));
vi.mock("../dialogs/ApprovalDialog", () => ({ default: () => null }));
vi.mock("../dialogs/PlanPreviewDialog", () => ({ default: () => null }));
vi.mock("../dialogs/SettingsDialog", () => ({ default: () => null }));
vi.mock("../command/CommandPalette", () => ({ default: () => null }));
vi.mock("../feedback/Toaster", () => ({ default: () => null }));

// --- ChatArea mock: 暴露触发回调的按钮，模拟 InputPanel → ModelPalette 链路 ---
vi.mock("../chat/ChatArea", () => ({
  default: ({
    onAddProvider,
    onManageProviders,
  }: {
    onAddProvider?: () => void;
    onManageProviders?: () => void;
  }) => (
    <div data-testid="mock-chat-area">
      <button onClick={() => onAddProvider?.()}>mock-trigger-add</button>
      <button onClick={() => onManageProviders?.()}>mock-trigger-manage</button>
    </div>
  ),
}));

// --- ProviderForm mock (AddProviderDialog 依赖) ---
vi.mock("../provider/ProviderForm", () => ({
  default: () => <div data-testid="mock-provider-form" />,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: vi.fn(),
}));

import AppLayout from "./AppLayout";

describe("AppLayout", () => {
  beforeEach(() => {
    mockConfigStore.providers = [];
    mockConfigStore.currentProvider = "";
    mockConfigStore.loadProviders.mockReset();
    mockSessionStore.createSession.mockReset();
    mockSessionStore.createSession.mockResolvedValue(undefined);
    mockSessionStore.loadSessions.mockReset();
    mockSessionStore.loadSessions.mockResolvedValue(undefined);
    mockSessionStore.sessions = [];
    mockChatStore.clearMessages.mockReset();
    mockChatStore.triggerFocusInput.mockReset();
    mockChatStore.clearPlanContent.mockReset();
    mockChatStore.planContent = null;
    mockWorkspaceStore.loadWorkspace.mockReset();
    mockWorkspaceStore.loadWorkspace.mockResolvedValue(undefined);
    mockWorkspaceStore.setWorkspace.mockReset();
    mockWorkspaceStore.setWorkspace.mockResolvedValue(undefined);
    mockWorkspaceStore.workspace = null;
    mockToastStore.addToast.mockReset();
  });

  it("renders AddProviderDialog at root level when add callback triggered", () => {
    render(<AppLayout />);

    // 初始时 AddProviderDialog 不应打开
    expect(screen.queryByText("连接提供商")).not.toBeInTheDocument();

    // 触发 add 回调
    fireEvent.click(screen.getByText("mock-trigger-add"));

    // AddProviderDialog 应在 AppLayout 根层打开（标题 "连接提供商" 出现）
    expect(screen.getByText("连接提供商")).toBeInTheDocument();
  });

  it("renders ModelManagerDialog at root level when manage callback triggered", () => {
    render(<AppLayout />);

    expect(screen.queryByText("管理模型")).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("mock-trigger-manage"));

    // ModelManagerDialog 应在 AppLayout 根层打开（标题 "管理模型" 出现）
    expect(screen.getByText("管理模型")).toBeInTheDocument();
  });

  it("opens AddProviderDialog from ModelManagerDialog add button", () => {
    render(<AppLayout />);

    // 先打开管理弹窗
    fireEvent.click(screen.getByText("mock-trigger-manage"));
    expect(screen.getByText("管理模型")).toBeInTheDocument();

    // 点击管理弹窗中的 "连接提供商" 按钮（Plus 按钮）
    fireEvent.click(screen.getByRole("button", { name: "连接提供商" }));

    // AddProviderDialog 应打开
    expect(screen.getByText("连接提供商")).toBeInTheDocument();
  });

  it("AddProviderDialog has dialog role and aria-modal when open", () => {
    render(<AppLayout />);
    fireEvent.click(screen.getByText("mock-trigger-add"));

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAccessibleName("连接提供商");
  });
});
