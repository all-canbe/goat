from __future__ import annotations

import time
from collections import OrderedDict
from dataclasses import dataclass, field


@dataclass
class _Bucket:
    tokens: float
    last_touched_ms: float = field(default_factory=lambda: time.time() * 1000)


class TokenBucketRateLimiter:
    def __init__(
        self,
        limit: int = 30,
        window_ms: float = 60_000,
        lru_cap: int = 10_000,
        ttl_ms: float = 300_000,
    ):
        self._limit = limit
        self._window_ms = window_ms
        self._lru_cap = lru_cap
        self._ttl_ms = ttl_ms
        self._buckets: OrderedDict[str, _Bucket] = OrderedDict()

    def check(self, key: str) -> tuple[bool, float]:
        now_ms = time.time() * 1000
        self._prune(now_ms)

        bucket = self._buckets.get(key)
        if bucket is None:
            bucket = _Bucket(tokens=self._limit)
            self._buckets[key] = bucket
        else:
            self._buckets.move_to_end(key)

        elapsed = now_ms - bucket.last_touched_ms
        if elapsed > 0:
            bucket.tokens = min(
                self._limit,
                bucket.tokens + (elapsed / self._window_ms) * self._limit,
            )
        bucket.last_touched_ms = now_ms

        if bucket.tokens >= 1:
            bucket.tokens -= 1
            self._evict_if_over()
            return True, 0.0

        retry_after = self._window_ms / 1000 * (1 - bucket.tokens / self._limit)
        self._evict_if_over()
        return False, max(retry_after, 0.0)

    def _prune(self, now_ms: float) -> None:
        expired = []
        for key, bucket in self._buckets.items():
            if now_ms - bucket.last_touched_ms > self._ttl_ms:
                expired.append(key)
            else:
                break
        for key in expired:
            del self._buckets[key]

    def _evict_if_over(self) -> None:
        while len(self._buckets) > self._lru_cap:
            self._buckets.popitem(last=False)

    @property
    def size(self) -> int:
        return len(self._buckets)