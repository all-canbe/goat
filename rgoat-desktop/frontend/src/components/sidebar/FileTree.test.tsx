import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// tauriInvoke mock：list_workspace_files 返回值可定制
const tauriInvokeMock = vi.hoisted(() => vi.fn());

// workspaceStore mock：默认无 workspace
const mockWorkspaceStore = vi.hoisted(() => ({
  workspace: null as { path: string; is_temporary: boolean } | null,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  tauriInvoke: tauriInvokeMock,
}));

vi.mock("../../stores/workspaceStore", () => ({
  useWorkspaceStore: (selector?: (s: typeof mockWorkspaceStore) => unknown) =>
    selector ? selector(mockWorkspaceStore) : mockWorkspaceStore,
}));

import FileTree from "./FileTree";

describe("FileTree", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    mockWorkspaceStore.workspace = null;
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
});
