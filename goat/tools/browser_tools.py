from __future__ import annotations

from langchain_core.tools import tool

from goat.browser.playwright_manager import PlaywrightManager


_manager: PlaywrightManager | None = None


def _get_manager() -> PlaywrightManager:
    global _manager
    if _manager is None:
        _manager = PlaywrightManager()
    return _manager


@tool
async def web_run(url: str, js_code: str = "", timeout: int = 30) -> str:
    """在浏览器中打开 URL 并可选执?JavaScript 代码，返回页面内容?
    类似 Playwright ?page.goto + page.evaluate?    适合需要渲?JS 的页面、动态内容抓取、或执行浏览器内 JS 脚本?
    Args:
        url: 要打开的网?URL（必须以 http:// ?https:// 开头）
        js_code: 可选，要在页面中执行的 JavaScript 代码
        timeout: 页面加载超时秒数，默?30，最?60
    """
    timeout = min(timeout, 60)
    manager = _get_manager()
    return await manager.run_js(url, js_code=js_code, timeout=timeout)


@tool
async def web_screenshot(url: str, timeout: int = 30) -> str:
    """在浏览器中打开 URL 并截取页面截图，保存?screenshots/ 目录?
    Args:
        url: 要截图的网页 URL（必须以 http:// ?https:// 开头）
        timeout: 页面加载超时秒数，默?30，最?60
    """
    timeout = min(timeout, 60)
    manager = _get_manager()
    return await manager.screenshot(url, timeout=timeout)