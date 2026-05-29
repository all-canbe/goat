from __future__ import annotations

import asyncio
import re
from typing import Optional

from ..core.paths import SCREENSHOTS_DIR


_URL_WHITELIST_RE = re.compile(r"^https?://", re.IGNORECASE)


class PlaywrightManager:
    def __init__(self):
        self._browser = None
        self._playwright = None
        self._context = None
        self._page_count = 0
        self._max_pages = 3
        self._idle_task: Optional[asyncio.Task] = None
        self._cleanup_interval = 300

    async def ensure_browser(self):
        if self._browser is not None:
            return True
        try:
            from playwright.async_api import async_playwright
            self._playwright = await async_playwright().start()
            self._browser = await self._playwright.chromium.launch(
                headless=True,
                args=[
                    "--no-sandbox",
                    "--disable-setuid-sandbox",
                    "--disable-dev-shm-usage",
                    "--disable-gpu",
                ],
            )
            self._context = await self._browser.new_context(
                viewport={"width": 1280, "height": 720},
                user_agent="GoatTUI/1.0",
            )
            self._page_count = 0
            return True
        except ImportError:
            return False
        except Exception:
            return False

    async def run_js(self, url: str, js_code: str = "", timeout: int = 30) -> str:
        if not _URL_WHITELIST_RE.match(url):
            return f"错误: 不支持的 URL 协议: {url[:50]}"

        if not await self.ensure_browser():
            return "错误: Playwright 未安装。请运行: pip install playwright && playwright install chromium"

        if self._page_count >= self._max_pages:
            return f"错误: 超过最大标签页数 ({self._max_pages})"

        page = await self._context.new_page()
        self._page_count += 1

        try:
            await page.goto(url, wait_until="domcontentloaded", timeout=timeout * 1000)

            result_parts = []
            if js_code:
                try:
                    js_result = await page.evaluate(js_code, timeout=10000)
                    result_parts.append(f"JS 结果: {js_result}")
                except Exception as e:
                    result_parts.append(f"JS 执行错误: {e}")

            title = await page.title()
            content = await page.evaluate("document.body?.innerText || ''")
            text_preview = content[:3000].strip()
            if text_preview:
                result_parts.append(f"页面文本:\n{text_preview}")
                if len(content) > 3000:
                    result_parts.append(f"\n...(共 {len(content)} 字符)")
            else:
                html_preview = await page.evaluate("document.body?.innerHTML?.substring(0, 500) || ''")
                if html_preview:
                    result_parts.append(f"页面 HTML 预览:\n{html_preview}")

            return f"标题: {title}\n" + "\n".join(result_parts)

        except Exception as e:
            return f"页面加载错误: {e}"
        finally:
            await page.close()
            self._page_count -= 1

    async def screenshot(self, url: str, timeout: int = 30) -> str:
        if not _URL_WHITELIST_RE.match(url):
            return f"错误: 不支持的 URL 协议: {url[:50]}"

        if not await self.ensure_browser():
            return "错误: Playwright 未安装"

        if self._page_count >= self._max_pages:
            return f"错误: 超过最大标签页数 ({self._max_pages})"

        page = await self._context.new_page()
        self._page_count += 1

        try:
            await page.goto(url, wait_until="domcontentloaded", timeout=timeout * 1000)
            import tempfile
            screenshot_dir = SCREENSHOTS_DIR
            import time
            filename = f"screenshot_{int(time.time())}.png"
            filepath = str(screenshot_dir / filename)
            await page.screenshot(path=filepath, full_page=False)
            return f"截图已保存: {filepath}"
        except Exception as e:
            return f"截图错误: {e}"
        finally:
            await page.close()
            self._page_count -= 1

    async def cleanup(self):
        if self._browser:
            try:
                await self._browser.close()
            except Exception:
                pass
            self._browser = None
        if self._playwright:
            try:
                await self._playwright.stop()
            except Exception:
                pass
            self._playwright = None
        self._context = None
        self._page_count = 0

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        await self.cleanup()