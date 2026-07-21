import { describe, it, expect, vi, beforeEach } from "vitest";

// tauriInvoke mock：每个测试可定制返回值/抛错
const tauriInvokeMock = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauri-bridge", () => ({
  tauriInvoke: tauriInvokeMock,
}));

import { useWorkspaceStore, type WorkspaceInfo } from "./workspaceStore";

describe("workspaceStore", () => {
  beforeEach(() => {
    tauriInvokeMock.mockReset();
    // 重置 store 状态
    useWorkspaceStore.setState({ workspace: null });
  });

  it("loadWorkspace 调用 get_workspace 并写入 workspace", async () => {
    const info: WorkspaceInfo = { path: "/tmp/abc", is_temporary: true };
    tauriInvokeMock.mockResolvedValueOnce(info);

    await useWorkspaceStore.getState().loadWorkspace();

    expect(tauriInvokeMock).toHaveBeenCalledWith("get_workspace");
    expect(useWorkspaceStore.getState().workspace).toEqual(info);
  });

  it("setWorkspace 调用 set_workspace 并替换 workspace", async () => {
    // 先 load 一个初始 workspace
    const initial: WorkspaceInfo = { path: "/tmp/old", is_temporary: true };
    tauriInvokeMock.mockResolvedValueOnce(initial);
    await useWorkspaceStore.getState().loadWorkspace();

    // setWorkspace 成功替换
    const next: WorkspaceInfo = { path: "/home/user/proj", is_temporary: false };
    tauriInvokeMock.mockResolvedValueOnce(next);

    await useWorkspaceStore.getState().setWorkspace("/home/user/proj");

    expect(tauriInvokeMock).toHaveBeenCalledWith("set_workspace", {
      path: "/home/user/proj",
    });
    expect(useWorkspaceStore.getState().workspace).toEqual(next);
  });

  it("setWorkspace 失败时抛出错误给 UI", async () => {
    tauriInvokeMock.mockRejectedValueOnce(
      new Error("Cannot switch workspace while an agent is running")
    );

    await expect(
      useWorkspaceStore.getState().setWorkspace("/some/path")
    ).rejects.toThrow("Cannot switch workspace while an agent is running");

    // 失败时不应改写 workspace
    expect(useWorkspaceStore.getState().workspace).toBeNull();
  });

  it("loadWorkspace 失败时抛出错误给 UI", async () => {
    tauriInvokeMock.mockRejectedValueOnce(new Error("backend unavailable"));

    await expect(useWorkspaceStore.getState().loadWorkspace()).rejects.toThrow(
      "backend unavailable"
    );

    expect(useWorkspaceStore.getState().workspace).toBeNull();
  });
});
