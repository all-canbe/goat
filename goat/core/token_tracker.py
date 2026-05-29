from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any

PRICING_TABLE: dict[str, dict[str, float]] = {
    # OpenAI 系列
    "gpt-4o": {"input": 2.50, "output": 10.00},
    "gpt-4o-mini": {"input": 0.15, "output": 0.60},
    "gpt-4-turbo": {"input": 10.00, "output": 30.00},
    "gpt-3.5-turbo": {"input": 0.50, "output": 1.50},
    # DeepSeek 系列
    "deepseek-v4-pro": {"input": 1.74, "output": 3.48},
    "deepseek-v4-flash": {"input": 0.14, "output": 0.28},
    "deepseek-chat": {"input": 0.14, "output": 0.28},
    "deepseek-coder": {"input": 0.14, "output": 0.28},
    # Anthropic 系列
    "claude-sonnet-4-20250514": {"input": 3.00, "output": 15.00},
    "claude-sonnet-4": {"input": 3.00, "output": 15.00},
    "claude-3-5-sonnet-20241022": {"input": 3.00, "output": 15.00},
    "claude-3-haiku-20240307": {"input": 0.25, "output": 1.25},
    "claude-opus-4": {"input": 15.00, "output": 75.00},
}

FALLBACK_PRICE: dict[str, float] = {"input": 1.00, "output": 2.00}


def _find_model_pricing(model: str) -> dict[str, float] | None:
    lower = model.lower().strip()
    if lower in PRICING_TABLE:
        return PRICING_TABLE[lower]
    for key, price in PRICING_TABLE.items():
        if key in lower or lower in key:
            return price
    return None


def get_pricing(model: str) -> dict[str, float]:
    pricing = _find_model_pricing(model)
    if pricing is not None:
        return pricing
    if any(name in model.lower() for name in ("gpt", "o1", "o3")):
        return PRICING_TABLE.get("gpt-4o-mini", FALLBACK_PRICE)
    if "claude" in model.lower():
        return PRICING_TABLE.get("claude-sonnet-4", FALLBACK_PRICE)
    if "deepseek" in model.lower():
        return PRICING_TABLE.get("deepseek-chat", FALLBACK_PRICE)
    return dict(FALLBACK_PRICE)


def estimate_tokens(text: str) -> int:
    if not text:
        return 0
    return max(1, len(text) // 4)


def calculate_cost(model: str, input_tokens: int, output_tokens: int) -> dict[str, float]:
    pricing = get_pricing(model)
    input_cost = (input_tokens / 1_000_000) * pricing["input"]
    output_cost = (output_tokens / 1_000_000) * pricing["output"]
    return {
        "input_cost": round(input_cost, 6),
        "output_cost": round(output_cost, 6),
        "total_cost": round(input_cost + output_cost, 6),
        "input_rate": pricing["input"],
        "output_rate": pricing["output"],
    }


def format_cost(cost: float) -> str:
    if cost < 0.0001:
        return "<$0.0001"
    elif cost < 0.01:
        return f"${cost:.4f}"
    else:
        return f"${cost:.2f}"


@dataclass
class TurnRecord:
    model: str
    input_tokens: int
    output_tokens: int
    input_cost: float
    output_cost: float
    total_cost: float
    timestamp: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())


class TokenTracker:
    def __init__(self, model: str = ""):
        self._model = model
        self._turns: list[TurnRecord] = []
        self._total_input_tokens: int = 0
        self._total_output_tokens: int = 0
        self._total_cost: float = 0.0

    @property
    def model(self) -> str:
        return self._model

    @model.setter
    def model(self, value: str) -> None:
        self._model = value

    @property
    def total_input_tokens(self) -> int:
        return self._total_input_tokens

    @property
    def total_output_tokens(self) -> int:
        return self._total_output_tokens

    @property
    def total_cost(self) -> float:
        return self._total_cost

    @property
    def turn_count(self) -> int:
        return len(self._turns)

    def record_turn(self, input_text: str, output_text: str) -> TurnRecord:
        input_tokens = estimate_tokens(input_text)
        output_tokens = estimate_tokens(output_text)
        cost_info = calculate_cost(self._model, input_tokens, output_tokens)

        record = TurnRecord(
            model=self._model,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            input_cost=cost_info["input_cost"],
            output_cost=cost_info["output_cost"],
            total_cost=cost_info["total_cost"],
        )
        self._turns.append(record)
        self._total_input_tokens += input_tokens
        self._total_output_tokens += output_tokens
        self._total_cost += cost_info["total_cost"]
        return record

    def record_llm_call(self, input_text: str = "", output_text: str = "",
                        input_tokens: int = 0, output_tokens: int = 0) -> TurnRecord:
        if input_tokens > 0 and output_tokens > 0:
            actual_input = input_tokens
            actual_output = output_tokens
        else:
            actual_input = estimate_tokens(input_text)
            actual_output = estimate_tokens(output_text)

        cost_info = calculate_cost(self._model, actual_input, actual_output)
        record = TurnRecord(
            model=self._model,
            input_tokens=actual_input,
            output_tokens=actual_output,
            input_cost=cost_info["input_cost"],
            output_cost=cost_info["output_cost"],
            total_cost=cost_info["total_cost"],
        )
        self._turns.append(record)
        self._total_input_tokens += actual_input
        self._total_output_tokens += actual_output
        self._total_cost += cost_info["total_cost"]
        return record

    def summary(self) -> dict[str, Any]:
        return {
            "model": self._model,
            "turns": self.turn_count,
            "total_input_tokens": self._total_input_tokens,
            "total_output_tokens": self._total_output_tokens,
            "total_tokens": self._total_input_tokens + self._total_output_tokens,
            "total_cost": round(self._total_cost, 6),
            "total_cost_str": format_cost(self._total_cost),
            "input_rate": get_pricing(self._model)["input"],
            "output_rate": get_pricing(self._model)["output"],
        }

    def get_recent_turns(self, n: int = 5) -> list[TurnRecord]:
        return self._turns[-n:]

    def reset(self) -> None:
        self._turns.clear()
        self._total_input_tokens = 0
        self._total_output_tokens = 0
        self._total_cost = 0.0