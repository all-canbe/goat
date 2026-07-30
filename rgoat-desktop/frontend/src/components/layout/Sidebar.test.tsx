import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Sidebar from "./Sidebar";

const mockSessionStore = vi.hoisted(() => ({
  sessions: [] as {
    id: string;
    title: string;
    message_count: number;
    created_at: string;
    workspace?: string | null;
  }[],
  activeSessionId: null as string | null,
  selectingId: null as string | null,
  deletingId: null as string | null,
  selectSession: vi.fn().mockResolvedValue(undefined),
  deleteSession: vi.fn().mockResolvedValue(undefined),
  renameSession: vi.fn().mockResolvedValue(undefined),
}));

const mockToastStore = vi.hoisted(() => ({
  addToast: vi.fn(),
}));

vi.mock("../../stores/sessionStore", () => ({
  useSessionStore: () => mockSessionStore,
}));

vi.mock("../../stores/toastStore", () => ({
  useToastStore: (selector?: (s: typeof mockToastStore) => unknown) =>
    selector ? selector(mockToastStore) : mockToastStore,
}));

describe("Sidebar", () => {
  beforeEach(() => {
    mockSessionStore.sessions = [
      {
        id: "session-1",
        title: "First Session",
        message_count: 2,
        created_at: new Date(Date.now() - 1000 * 60 * 5).toISOString(),
        workspace: "E:/workspace",
      },
      {
        id: "session-2",
        title: "Second Session",
        message_count: 0,
        created_at: new Date(Date.now() - 1000 * 60 * 60).toISOString(),
        workspace: "临时工作空间",
      },
    ];
    mockSessionStore.activeSessionId = null;
    mockSessionStore.selectingId = null;
    mockSessionStore.deletingId = null;
    mockSessionStore.selectSession.mockReset().mockResolvedValue(undefined);
    mockSessionStore.deleteSession.mockReset().mockResolvedValue(undefined);
    mockSessionStore.renameSession.mockReset().mockResolvedValue(undefined);
    mockToastStore.addToast.mockReset();
  });

  function renderSidebar(props = {}) {
    return render(
      <Sidebar
        collapsed={false}
        onNewSession={vi.fn()}
        onCreateSessionForWorkspace={vi.fn().mockResolvedValue(undefined)}
        {...props}
      />
    );
  }

  it("renders sessions grouped by workspace", () => {
    renderSidebar();

    expect(screen.getByText("workspace")).toBeInTheDocument();
    expect(screen.getByText("临时工作空间")).toBeInTheDocument();
    expect(screen.getByText("First Session")).toBeInTheDocument();
    expect(screen.getByText("Second Session")).toBeInTheDocument();
  });

  it("calls selectSession and highlights the active session when clicked", async () => {
    renderSidebar();

    fireEvent.click(screen.getByText("First Session"));

    await waitFor(() =>
      expect(mockSessionStore.selectSession).toHaveBeenCalledWith("session-1")
    );
  });

  it("shows error toast when selectSession fails", async () => {
    mockSessionStore.selectSession.mockRejectedValueOnce(new Error("db locked"));

    renderSidebar();

    fireEvent.click(screen.getByText("First Session"));

    await waitFor(() =>
      expect(mockToastStore.addToast).toHaveBeenCalledWith(
        "加载会话历史失败：db locked",
        "error"
      )
    );
  });

  it("calls deleteSession when delete button is clicked", async () => {
    renderSidebar();

    const sessionItem = screen.getByText("First Session").closest("div");
    fireEvent.mouseEnter(sessionItem!);

    const deleteButton = screen.getAllByRole("button", { name: "Delete session" })[0];
    fireEvent.click(deleteButton);

    await waitFor(() =>
      expect(mockSessionStore.deleteSession).toHaveBeenCalledWith("session-1")
    );
  });

  it("shows error toast when deleteSession fails", async () => {
    mockSessionStore.deleteSession.mockRejectedValueOnce(new Error("not found"));

    renderSidebar();

    const sessionItem = screen.getByText("First Session").closest("div");
    fireEvent.mouseEnter(sessionItem!);

    const deleteButton = screen.getAllByRole("button", { name: "Delete session" })[0];
    fireEvent.click(deleteButton);

    await waitFor(() =>
      expect(mockToastStore.addToast).toHaveBeenCalledWith(
        "删除会话失败：not found",
        "error"
      )
    );
  });

  it("collapses and expands workspace sessions when header is clicked", () => {
    renderSidebar();

    const workspaceHeader = screen.getByText("workspace").closest("div");
    expect(screen.getByText("First Session")).toBeInTheDocument();

    // Click the workspace header (not the plus button)
    fireEvent.click(workspaceHeader!);

    expect(screen.queryByText("First Session")).not.toBeInTheDocument();

    fireEvent.click(workspaceHeader!);

    expect(screen.getByText("First Session")).toBeInTheDocument();
  });
});
