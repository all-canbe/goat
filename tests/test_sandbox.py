#!/usr/bin/env python3
"""
沙箱功能测试脚本 -- 验证各平台沙箱是否正常工�?
运行方式:
    python test_sandbox.py                          # 运行所有测�?    python test_sandbox.py windows                   # �?Windows 沙箱测试
    python test_sandbox.py async_executor            # 仅异步执行器测试
    python test_sandbox.py integration               # 仅集成测�?
测试�?
    1. 沙箱创建工厂 -- 验证 create_sandbox() 返回正确平台实例
    2. 沙箱执行 -- 验证沙箱能正常执行命�?    3. 沙箱终止 -- 验证 terminate() 能杀死进�?    4. 超时机制 -- 验证超时后能正确终止
    5. 异步执行�?-- 验证 stdout/stderr 回调能收到输�?    6. 集成测试 -- 验证 async_execute_command 走沙箱路�?    7. 沙箱限制验证 (Windows) -- Job Object 能否限制子进�?    8. EventBus 工具事件 -- 验证 TOOL_STDOUT/STDERR 事件发布
"""

import asyncio
import io
import os
import sys
import time

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')

PASS = 0
FAIL = 0


def _log(msg: str):
    print(f"  {msg}")


def _check(name: str, condition: bool, detail: str = ""):
    global PASS, FAIL
    if condition:
        PASS += 1
        print(f"  [PASS] {name}")
    else:
        FAIL += 1
        print(f"  [FAIL] {name} -- {detail}")


# ============================================================
# 1. 沙箱创建工厂测试
# ============================================================

def test_sandbox_factory():
    print("\n=== 1. 沙箱创建工厂测试 ===")
    from goat.security.sandbox import create_sandbox

    sandbox = create_sandbox()
    _check("create_sandbox 返回 BaseSandbox 实例",
           hasattr(sandbox, "run") and hasattr(sandbox, "terminate") and hasattr(sandbox, "get_status"))

    status = sandbox.get_status()
    expected_type = "windows_job" if sys.platform == "win32" else ("macos" if sys.platform == "darwin" else "linux")
    _check(f"沙箱类型正确 ({status.get('type')})",
           status.get("type") == expected_type,
           f"期望 {expected_type}, 实际 {status.get('type')}")

    print(f"  沙箱状�? {status}")


# ============================================================
# 2. 沙箱执行命令测试
# ============================================================

async def test_sandbox_execute():
    print("\n=== 2. 沙箱执行命令测试 ===")
    from goat.security.sandbox import create_sandbox

    sandbox = create_sandbox()

    echo_cmd = 'echo "hello sandbox"'
    result = await sandbox.run(echo_cmd)
    _check("沙箱 echo 命令返回码为 0", result.returncode == 0,
           f"实际 returncode: {result.returncode}")
    _check("沙箱 echo 输出包含 hello sandbox",
           "hello sandbox" in result.stdout,
           f"实际 stdout: {result.stdout}")
    _check("沙箱类型已标记", result.sandbox_type != "none",
           f"类型: {result.sandbox_type}")

    wd = os.getcwd()
    result2 = await sandbox.run('echo "cwd test"', working_dir=wd)
    _check("沙箱指定工作目录执行成功", result2.returncode == 0)

    result3 = await sandbox.run("nonexistent_command_xyz123")
    _check("不存在命令返回非零返回码", result3.returncode != 0,
           f"returncode: {result3.returncode}")


# ============================================================
# 3. 沙箱终止测试
# ============================================================

async def test_sandbox_terminate():
    print("\n=== 3. 沙箱终止测试 ===")
    from goat.security.sandbox import create_sandbox

    sandbox = create_sandbox()

    if sys.platform == "win32":
        sleep_cmd = "ping -n 60 127.0.0.1"
    else:
        sleep_cmd = "sleep 60"

    try:
        proc_task = asyncio.create_task(sandbox.run(sleep_cmd, timeout=30))
        await asyncio.sleep(0.5)

        sandbox.terminate()
        result = await asyncio.wait_for(proc_task, timeout=5)
        _check("沙箱 terminate 后进程终止", result.terminated or result.returncode != 0,
               f"terminated={result.terminated}, returncode={result.returncode}")
    except asyncio.TimeoutError:
        sandbox.terminate()
        _check("沙箱 terminate 超时（需检查）", False, "进程未能及时终止")
    except Exception as e:
        _check("沙箱 terminate 异常", False, str(e))


# ============================================================
# 4. 超时测试
# ============================================================

async def test_sandbox_timeout():
    print("\n=== 4. 超时机制测试 ===")
    from goat.security.sandbox import create_sandbox

    sandbox = create_sandbox()

    if sys.platform == "win32":
        long_cmd = "ping -n 30 127.0.0.1"
    else:
        long_cmd = "sleep 30"

    start = time.time()
    result = await sandbox.run(long_cmd, timeout=3)
    elapsed = time.time() - start

    _check("超时后返回码�?-1", result.returncode == -1,
           f"returncode: {result.returncode}")
    _check("超时�?terminated �?True", result.terminated,
           f"terminated: {result.terminated}")
    _check(f"实际耗时 ({elapsed:.1f}s) 小于 10s", elapsed < 10,
           f"耗时 {elapsed:.1f}s，可能超时机制失效")


# ============================================================
# 5. 异步执行器测�?# ============================================================

async def test_async_executor():
    print("\n=== 5. 异步执行器测�?===")
    from goat.tools.async_executor import execute_command_async

    stdout_lines = []
    returncode, stdout, stderr = await execute_command_async(
        'cmd /c echo line1 & echo line2' if sys.platform == "win32"
        else 'echo "line1" && echo "line2"',
        on_stdout=lambda l: stdout_lines.append(l),
    )
    _check("async_executor 返回码为 0", returncode == 0,
           f"returncode: {returncode}")
    _check("async_executor stdout 包含 line1", "line1" in stdout)
    _check("async_executor stdout 包含 line2", "line2" in stdout)
    _check(f"回调收到 {len(stdout_lines)} 行输出", len(stdout_lines) >= 1,
           f"实际行数: {len(stdout_lines)}")

    if sys.platform == "win32":
        timeout_cmd = "ping -n 30 127.0.0.1"
    else:
        timeout_cmd = "sleep 30"
    start = time.time()
    rc, _, _ = await execute_command_async(timeout_cmd, timeout=3)
    elapsed = time.time() - start
    _check(f"async_executor 超时返回 -1 (耗时 {elapsed:.1f}s)",
           rc == -1, f"returncode: {rc}")
    _check(f"超时耗时 ({elapsed:.1f}s) 小于 10s", elapsed < 10,
           f"超时机制可能失效，耗时 {elapsed:.1f}s")

    multiline_out = []
    rc2, stdout2, _ = await execute_command_async(
        'cmd /c "for /l %i in (1,1,3) do @echo line %i"' if sys.platform == "win32"
        else 'for i in 1 2 3; do echo "line $i"; done',
        on_stdout=lambda l: multiline_out.append(l),
    )
    _check("多行命令返回码为 0", rc2 == 0, f"returncode: {rc2}")


# ============================================================
# 6. 集成测试 -- async_execute_command tool
# ============================================================

async def test_integration():
    print("\n=== 6. 集成测试 ===")
    from goat.tools.tools import async_execute_command

    result = await async_execute_command.ainvoke({
        "command": 'cmd /c echo integration test' if sys.platform == "win32" else 'echo "integration test"',
    })
    _check("集成工具返回包含 integration test",
           "integration test" in result,
           f"实际: {result[:100]}")

    result_blocked = await async_execute_command.ainvoke({
        "command": "sudo echo test",
    })
    _check("黑名单阻�?sudo", "安全拒绝" in result_blocked,
           f"实际: {result_blocked[:100]}")

    result_escape = await async_execute_command.ainvoke({
        "command": "cd /etc && ls",
    })
    _check("逃逸检测阻�?cd /etc", "安全拒绝" in result_escape,
           f"实际: {result_escape[:100]}")

    if sys.platform == "win32":
        timeout_cmd = "ping -n 30 127.0.0.1"
    else:
        timeout_cmd = "sleep 30"
    result_timeout = await async_execute_command.ainvoke({
        "command": timeout_cmd, "timeout": 3,
    })
    _check("集成工具超时", "超时" in result_timeout,
           f"实际: {result_timeout[:100]}")


# ============================================================
# 7. Windows Job Object 限制验证 (�?Windows)
# ============================================================

async def test_windows_job_limits():
    print("\n=== 7. Windows Job Object 限制验证 ===")
    if sys.platform != "win32":
        print("  跳过: 仅在 Windows 平台运行")
        return

    from goat.security.sandbox import create_sandbox
    sandbox = create_sandbox()
    status = sandbox.get_status()

    _check("Windows Job Object 已启用", status.get("enabled", False),
           f"status: {status}")

    try:
        tasks = []
        for i in range(5):
            cmd = 'ping -n 10 127.0.0.1'
            tasks.append(asyncio.create_task(sandbox.run(cmd, timeout=5)))
        results = await asyncio.gather(*tasks, return_exceptions=True)
        success_count = sum(1 for r in results if isinstance(r, object) and hasattr(r, 'returncode') and r.returncode == 0)
        _check(f"Job Object 并发进程限制 ({success_count}/5 成功)",
               success_count < 5,
               f"ActiveProcessLimit=3 应限制部分进程")
    except Exception as e:
        _check("Job Object 多进程测试", False, str(e))


# ============================================================
# 8. EventBus 工具事件测试
# ============================================================

async def test_eventbus_tool_events():
    print("\n=== 8. EventBus 工具事件测试 ===")
    from goat.core.event_bus import EventBus, EventType

    bus = EventBus()
    queue = await bus.stream("test_sub")
    received_events = []

    async def listener():
        while True:
            try:
                event = await asyncio.wait_for(queue.get(), timeout=3)
                received_events.append(event)
                queue.task_done()
            except asyncio.TimeoutError:
                break

    listener_task = asyncio.create_task(listener())
    await asyncio.sleep(0.1)

    bus.publish_nowait("test", EventType.TOOL_STDOUT, "hello stdout", "tool")
    bus.publish_nowait("test", EventType.TOOL_STDERR, "hello stderr", "tool")
    bus.publish_nowait("test", EventType.TOOL_CALL,
                       '{"tool_name": "execute_command", "args": {"command": "test"}}', "tool")
    bus.publish_nowait("test", EventType.TOOL_RESULT,
                       '{"tool_name": "execute_command", "result": "done"}', "tool")

    await asyncio.sleep(0.2)
    listener_task.cancel()

    stdout_events = [e for e in received_events if e.event_type == EventType.TOOL_STDOUT]
    stderr_events = [e for e in received_events if e.event_type == EventType.TOOL_STDERR]
    tool_call_events = [e for e in received_events if e.event_type == EventType.TOOL_CALL]
    tool_result_events = [e for e in received_events if e.event_type == EventType.TOOL_RESULT]

    _check("收到 TOOL_STDOUT 事件", len(stdout_events) >= 1,
           f"实际数量: {len(stdout_events)}")
    _check("收到 TOOL_STDERR 事件", len(stderr_events) >= 1,
           f"实际数量: {len(stderr_events)}")
    _check("收到 TOOL_CALL 事件", len(tool_call_events) >= 1,
           f"实际数量: {len(tool_call_events)}")
    _check("TOOL_CALL payload 可解析",
           "execute_command" in (tool_call_events[0].payload if tool_call_events else ""))
    _check("收到 TOOL_RESULT 事件", len(tool_result_events) >= 1)


# ============================================================
# 运行入口
# ============================================================

async def run_all():
    global PASS, FAIL

    test_sandbox_factory()
    await test_sandbox_execute()
    await test_async_executor()
    await test_integration()
    await test_eventbus_tool_events()
    await test_windows_job_limits()
    await test_sandbox_terminate()
    await test_sandbox_timeout()

    print(f"\n{'='*50}")
    print(f"结果: {PASS} 通过, {FAIL} 失败")
    if FAIL > 0:
        print("[WARN] 有测试未通过，请检查日志")
        sys.exit(1)
    else:
        print("[OK] 全部测试通过")


async def main():
    args = sys.argv[1:] if len(sys.argv) > 1 else ["all"]

    test_map = {
        "factory": test_sandbox_factory,
        "execute": test_sandbox_execute,
        "async_executor": test_async_executor,
        "integration": test_integration,
        "events": test_eventbus_tool_events,
        "windows": test_windows_job_limits,
        "terminate": test_sandbox_terminate,
        "timeout": test_sandbox_timeout,
    }

    if "all" in args:
        await run_all()
    else:
        for arg in args:
            if arg in test_map:
                fn = test_map[arg]
                if asyncio.iscoroutinefunction(fn):
                    await fn()
                else:
                    fn()
            else:
                print(f"未知测试: {arg}")
                print(f"可用: {', '.join(test_map.keys())}, all")

    print(f"\n结果: {PASS} 通过, {FAIL} 失败")


if __name__ == "__main__":
    asyncio.run(main())