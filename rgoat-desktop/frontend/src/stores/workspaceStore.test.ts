import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

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
    useWorkspaceStore.setState({ workspace: null, pendingWorkspacePath: null });
    useWorkspaceStore.getState().cancelPendingSwitch();
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

describe("workspaceStore.scheduleWorkspaceSwitch (5s debounce)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauriInvokeMock.mockReset();
    useWorkspaceStore.setState({ workspace: null, pendingWorkspacePath: null });
    useWorkspaceStore.getState().cancelPendingSwitch();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("5 秒内不调用 set_workspace，5 秒后调用", async () => {
    const next: WorkspaceInfo = { path: "/ws/a", is_temporary: false };
    tauriInvokeMock.mockResolvedValue(next);

    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/a");

    // 立即设置 pendingWorkspacePath
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBe("/ws/a");
    // 5 秒内不应调用 set_workspace
    expect(tauriInvokeMock).not.toHaveBeenCalled();

    // 推进 4.9 秒仍未调用
    await vi.advanceTimersByTimeAsync(4900);
    expect(tauriInvokeMock).not.toHaveBeenCalled();

    // 推进到 5 秒触发，并等待 async 回调完成
    await vi.advanceTimersByTimeAsync(100);

    expect(tauriInvokeMock).toHaveBeenCalledWith("set_workspace", { path: "/ws/a" });
    expect(useWorkspaceStore.getState().workspace).toEqual(next);
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBeNull();
  });

  it("连续切换 A→B（间隔 < 5s）：只对 B 调用 set_workspace", async () => {
    const infoB: WorkspaceInfo = { path: "/ws/b", is_temporary: false };
    tauriInvokeMock.mockResolvedValue(infoB);

    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/a");
    await vi.advanceTimersByTimeAsync(3000); // 3 秒后切到 B
    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/b");

    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBe("/ws/b");

    // 推进到原 A 的定时器时刻（自 B 起 2 秒）——不应触发
    await vi.advanceTimersByTimeAsync(2000);
    expect(tauriInvokeMock).not.toHaveBeenCalled();

    // 推进到 B 的 5 秒触发
    await vi.advanceTimersByTimeAsync(3000);

    expect(tauriInvokeMock).toHaveBeenCalledTimes(1);
    expect(tauriInvokeMock).toHaveBeenCalledWith("set_workspace", { path: "/ws/b" });
    expect(useWorkspaceStore.getState().workspace).toEqual(infoB);
  });

  it("path === null：立即清空 workspace，不启动定时器", async () => {
    const initial: WorkspaceInfo = { path: "/tmp/old", is_temporary: false };
    useWorkspaceStore.setState({ workspace: initial });

    useWorkspaceStore.getState().scheduleWorkspaceSwitch(null);

    expect(useWorkspaceStore.getState().workspace).toBeNull();
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBeNull();
    // 推进时间不应有任何 invoke
    await vi.advanceTimersByTimeAsync(10000);
    expect(tauriInvokeMock).not.toHaveBeenCalled();
  });

  it("path === 当前 workspace.path：不调用 set_workspace，清空 pending", async () => {
    const current: WorkspaceInfo = { path: "/ws/same", is_temporary: false };
    useWorkspaceStore.setState({ workspace: current, pendingWorkspacePath: "/ws/other" });

    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/same");

    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBeNull();
    expect(useWorkspaceStore.getState().workspace).toEqual(current);
    await vi.advanceTimersByTimeAsync(10000);
    expect(tauriInvokeMock).not.toHaveBeenCalled();
  });

  it("set_workspace 失败（agent 运行中）：清空 pending，保留当前 workspace", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const current: WorkspaceInfo = { path: "/ws/current", is_temporary: false };
    useWorkspaceStore.setState({ workspace: current });

    tauriInvokeMock.mockRejectedValueOnce(
      new Error("Cannot switch workspace while an agent is running")
    );

    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/new");
    await vi.advanceTimersByTimeAsync(5000);

    expect(tauriInvokeMock).toHaveBeenCalledWith("set_workspace", { path: "/ws/new" });
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBeNull();
    // 失败时保留原 workspace
    expect(useWorkspaceStore.getState().workspace).toEqual(current);
    expect(warnSpy).toHaveBeenCalled();
    warnSpy.mockRestore();
  });

  it("旧 RPC 竞态：A 的延迟响应不覆盖 B 的 pending", async () => {
    let resolveA!: (info: WorkspaceInfo) => void;
    const pendingA = new Promise<WorkspaceInfo>((resolve) => {
      resolveA = resolve;
    });
    const infoA: WorkspaceInfo = { path: "/ws/a", is_temporary: false };
    const infoB: WorkspaceInfo = { path: "/ws/b", is_temporary: false };

    tauriInvokeMock.mockImplementation((_command: string, args?: { path: string }) => {
      if (args?.path === "/ws/a") return pendingA;
      return Promise.resolve(infoB);
    });

    // 调度 A
    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/a");
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBe("/ws/a");

    // 推进 5 秒触发 A 的定时器（A 的 IPC 挂在 pendingA 上）
    await vi.advanceTimersByTimeAsync(5000);
    // A 的 set_workspace 已被调用但未 resolve
    expect(tauriInvokeMock).toHaveBeenCalledWith("set_workspace", { path: "/ws/a" });

    // 调度 B（A 的 IPC 仍在飞行中，B 清除 A 的已触发定时器）
    useWorkspaceStore.getState().scheduleWorkspaceSwitch("/ws/b");
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBe("/ws/b");

    // 现在 resolve A 的 IPC —— A 的响应不应改写 workspace 或 pending
    resolveA(infoA);
    // 等待 microtask 刷新
    await vi.advanceTimersByTimeAsync(0);

    // A 的响应不应覆盖 B 的 pending
    expect(useWorkspaceStore.getState().pendingWorkspacePath).toBe("/ws/b");
    // workspace 不应被 A 写回
    expect(useWorkspaceStore.getState().workspace).toBeNull();
  });
});

