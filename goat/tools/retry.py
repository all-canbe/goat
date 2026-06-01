from __future__ import annotations

import asyncio
import functools
import time
from dataclasses import dataclass


_event_bus = None


def init_retry(event_bus):
    global _event_bus
    _event_bus = event_bus


@dataclass
class RetryConfig:
    max_retries: int = 3
    base_delay: float = 1.0
    max_delay: float = 8.0
    retryable_exceptions: tuple[type[Exception], ...] = ()


_NETWORK_RETRYABLE = (
    ConnectionError,
    OSError,
    TimeoutError,
)

try:
    import requests.exceptions
    _NETWORK_RETRYABLE = (
        *_NETWORK_RETRYABLE,
        requests.exceptions.ConnectionError,
        requests.exceptions.Timeout,
    )
except ImportError:
    pass


def _should_retry(exc: Exception, cfg: RetryConfig) -> bool:
    return isinstance(exc, cfg.retryable_exceptions)


def _calc_delay(attempt: int, cfg: RetryConfig) -> float:
    delay = cfg.base_delay * (2 ** attempt)
    return min(delay, cfg.max_delay)


def _notify_retry(tool_name: str, attempt: int, max_retries: int, delay: float, error: str):
    if _event_bus is None:
        return
    from goat.core.event_bus import EventType
    _event_bus.publish_nowait(
        "retry", EventType.TOOL_RETRY,
        f"🔄 {tool_name} 重试?({attempt}/{max_retries})，{delay:.1f}s 后重??{error}",
    )


def retry_sync(config: RetryConfig | None = None):
    cfg = config or RetryConfig(retryable_exceptions=_NETWORK_RETRYABLE)

    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            last_exc = None
            for attempt in range(cfg.max_retries + 1):
                try:
                    return func(*args, **kwargs)
                except Exception as e:
                    if not _should_retry(e, cfg) or attempt >= cfg.max_retries:
                        raise
                    last_exc = e
                    delay = _calc_delay(attempt, cfg)
                    _notify_retry(func.__name__, attempt + 1, cfg.max_retries, delay, str(e))
                    time.sleep(delay)
            raise last_exc
        return wrapper
    return decorator


def retry_async(config: RetryConfig | None = None):
    cfg = config or RetryConfig(retryable_exceptions=_NETWORK_RETRYABLE)

    def decorator(func):
        @functools.wraps(func)
        async def wrapper(*args, **kwargs):
            last_exc = None
            for attempt in range(cfg.max_retries + 1):
                try:
                    return await func(*args, **kwargs)
                except Exception as e:
                    if not _should_retry(e, cfg) or attempt >= cfg.max_retries:
                        raise
                    last_exc = e
                    delay = _calc_delay(attempt, cfg)
                    _notify_retry(func.__name__, attempt + 1, cfg.max_retries, delay, str(e))
                    await asyncio.sleep(delay)
            raise last_exc
        return wrapper
    return decorator
