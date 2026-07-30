import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// tauriInvoke mock：list_workspace_files 返回值可定制
const tauriInvokeMock = vi.hoisted(() => vi.fn());

// workspaceStore mock：默认无 workspace
const mockWorkspaceStore = vi.hoisted(() => ({
  workspace: null as { path: string; is_temporary: boolean } | null,
  previewTarget: null as { path: string; name: string } | null,
  openPreview: vi.fn(),
  closePreview: vi.fn(),
}));

const mockChatStore = vi.hoisted(() => ({
  fileRefs: [] as { id: string; name: string; path: string }[],
  addFileRef: vi.fn(),
  removeFileRef: vi.fn(),
  clearFileRefs: vi.fn(),
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: tauriInvokeMock,
}));

vi.mock("../../stores/workspaceStore", () => ({
  useWorkspaceStore: (selector?: (s: typeof mockWorkspaceStore) => unknown) =>
    selector ? selector(mockWorkspaceStore) : mockWorkspaceStore,
}));

vi.mock("../../stores/chatStore", () => ({
  useChatStore: (selector?: (s: typeof mockChatStore) => unknown) =>
    selector ? selector(mockChatStore) : mockChatStore,
}));

import FileTree from "./FileTree";

describe("FileTree", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    mockWorkspaceStore.workspace = null;
    mockChatStore.addFileRef.mockReset();
    mockWorkspaceStore.openPreview.mockReset();
    mockWorkspaceStore.previewTarget = null;
  });

  it("renders children from backend root node", async () => {
    // 后端返回单个 root 节点（新协议），其 children 含 file1.txt
    const root = {
      name: "proj",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "file1.txt", path: "/proj/file1.txt", is_dir: false },
        { name: "src", path: "/proj/src", is_dir: true, children: [] },
      ],
    };
    tauriInvokeMock.mockResolvedValueOnce(root);

    render(<FileTree max_depth={3} />);

    // 应渲染 root 的 children（file1.txt），而不是把 root 当作数组
    await waitFor(() => {
      expect(screen.getByText("file1.txt")).toBeInTheDocument();
    });
    expect(screen.getByText("src")).toBeInTheDocument();

    // 不应渲染 root 节点自身的 name（因为渲染的是 children）
    expect(screen.queryByText("proj")).not.toBeInTheDocument();
  });

  it("shows add workspace when current workspace is temporary", async () => {
    mockWorkspaceStore.workspace = {
      path: "/tmp/rgoat-xyz",
      is_temporary: true,
    };
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/tmp/rgoat-xyz",
      is_dir: true,
      children: [],
    });

    const onAddWorkspace = vi.fn();
    render(<FileTree max_depth={3} onAddWorkspace={onAddWorkspace} />);

    await waitFor(() => {
      expect(screen.getByText("临时工作空间")).toBeInTheDocument();
    });

    const btn = screen.getByRole("button", { name: "添加工作空间" });
    fireEvent.click(btn);
    expect(onAddWorkspace).toHaveBeenCalledTimes(1);
  });

  it("does not show add workspace button when workspace is not temporary", async () => {
    mockWorkspaceStore.workspace = {
      path: "/home/user/proj",
      is_temporary: false,
    };
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/home/user/proj",
      is_dir: true,
      children: [],
    });

    render(<FileTree max_depth={3} onAddWorkspace={vi.fn()} />);

    await waitFor(() => {
      expect(screen.queryByText("临时工作空间")).not.toBeInTheDocument();
    });
    expect(screen.queryByRole("button", { name: "添加工作空间" })).toBeNull();
  });

  it("shows context menu on file right-click", async () => {
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "test.ts", path: "test.ts", is_dir: false },
      ],
    });

    render(<FileTree max_depth={3} />);

    await waitFor(() => {
      expect(screen.getByText("test.ts")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("test.ts"));

    expect(screen.getByText("添加到对话")).toBeInTheDocument();
  });

  it("context menu shows preview for html files only", async () => {
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "index.html", path: "index.html", is_dir: false },
        { name: "app.ts", path: "app.ts", is_dir: false },
      ],
    });

    render(<FileTree max_depth={3} />);

    await waitFor(() => {
      expect(screen.getByText("index.html")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("index.html"));
    expect(screen.getByText("预览")).toBeInTheDocument();
    expect(screen.getByText("添加到对话")).toBeInTheDocument();

    fireEvent.click(document.body);

    fireEvent.contextMenu(screen.getByText("app.ts"));
    expect(screen.getByText("添加到对话")).toBeInTheDocument();
    expect(screen.queryByText("预览")).not.toBeInTheDocument();
  });

  it("addFileRef is called when clicking add to chat", async () => {
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "test.ts", path: "src/test.ts", is_dir: false },
      ],
    });

    render(<FileTree max_depth={3} />);

    await waitFor(() => {
      expect(screen.getByText("test.ts")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("test.ts"));
    fireEvent.click(screen.getByText("添加到对话"));

    expect(mockChatStore.addFileRef).toHaveBeenCalledWith({
      name: "test.ts",
      path: "src/test.ts",
    });
  });

  it("openPreview is called when clicking preview on html file", async () => {
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "index.html", path: "index.html", is_dir: false },
      ],
    });

    render(<FileTree max_depth={3} />);

    await waitFor(() => {
      expect(screen.getByText("index.html")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("index.html"));
    fireEvent.click(screen.getByText("预览"));

    expect(mockWorkspaceStore.openPreview).toHaveBeenCalledWith(
      "index.html",
      "index.html"
    );
  });

  it("lazy-loads subdirectory via list_directory on expand", async () => {
    // 根节点 children 含一个目录 src（children 为空，触发懒加载）
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "src", path: "src", is_dir: true, children: [] },
      ],
    });
    // list_directory 返回 src 下的文件
    tauriInvokeMock.mockResolvedValueOnce([
      { name: "index.ts", path: "src/index.ts", is_dir: false },
    ]);

    render(<FileTree max_depth={1} />);

    await waitFor(() => {
      expect(screen.getByText("src")).toBeInTheDocument();
    });

    // 点击展开 src，应触发 list_directory
    fireEvent.click(screen.getByText("src"));

    await waitFor(() => {
      expect(tauriInvokeMock).toHaveBeenCalledWith("list_directory", { path: "src" });
    });
    await waitFor(() => {
      expect(screen.getByText("index.ts")).toBeInTheDocument();
    });
  });

  it("shows loading indicator while lazy-loading subdirectory", async () => {
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "src", path: "src", is_dir: true, children: [] },
      ],
    });
    // 让 list_directory 永远 pending（不 resolve）
    tauriInvokeMock.mockImplementationOnce(() => new Promise(() => {}));

    render(<FileTree max_depth={1} />);

    await waitFor(() => {
      expect(screen.getByText("src")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("src"));

    await waitFor(() => {
      expect(screen.getByText("Loading...")).toBeInTheDocument();
    });
  });

  it("shows error message when list_directory fails", async () => {
    tauriInvokeMock.mockResolvedValueOnce({
      name: "root",
      path: "/proj",
      is_dir: true,
      children: [
        { name: "src", path: "src", is_dir: true, children: [] },
      ],
    });
    tauriInvokeMock.mockRejectedValueOnce(new Error("permission denied"));

    render(<FileTree max_depth={1} />);

    await waitFor(() => {
      expect(screen.getByText("src")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("src"));

    await waitFor(() => {
      expect(screen.getByText("permission denied")).toBeInTheDocument();
    });
  });
});
