from __future__ import annotations

from enum import Enum
from dataclasses import dataclass, field
from typing import Any
from abc import ABC, abstractmethod

from langchain_core.language_models.chat_models import BaseChatModel
from langchain_openai import ChatOpenAI

try:
    from langchain_anthropic import ChatAnthropic
    _HAS_ANTHROPIC = True
except ImportError:
    ChatAnthropic = None
    _HAS_ANTHROPIC = False

PROVIDER_DEFAULTS = {
    "openai_compatible": {
        "base_url": "https://api.openai.com/v1",
        "model": "gpt-4o",
    },
    "anthropic": {
        "base_url": "https://api.anthropic.com/v1",
        "model": "claude-sonnet-4-20250514",
    },
}


class ProviderType(Enum):
    OPENAI_COMPATIBLE = "openai_compatible"
    ANTHROPIC = "anthropic"

    @classmethod
    def _missing_(cls, value: str) -> ProviderType | None:
        normalized = value.lower().replace("-", "_").replace(" ", "_")
        for member in cls:
            if member.value == normalized:
                return member
        aliases = {
            "openai": cls.OPENAI_COMPATIBLE,
            "deepseek": cls.OPENAI_COMPATIBLE,
            "ollama": cls.OPENAI_COMPATIBLE,
            "claude": cls.ANTHROPIC,
        }
        return aliases.get(normalized)


PROVIDER_DISPLAY_NAMES: dict[ProviderType, str] = {
    ProviderType.OPENAI_COMPATIBLE: "OpenAI 兼容",
    ProviderType.ANTHROPIC: "Anthropic",
}

PROVIDER_SHORT_NAMES: dict[ProviderType, str] = {
    ProviderType.OPENAI_COMPATIBLE: "openai",
    ProviderType.ANTHROPIC: "anthropic",
}


@dataclass
class ProviderConfig:
    provider_type: ProviderType
    api_key: str
    model: str = ""
    base_url: str = ""
    temperature: float = 0.7
    max_tokens: int | None = None
    timeout: int | None = None

    def __post_init__(self) -> None:
        defaults = PROVIDER_DEFAULTS.get(self.provider_type.value, {})
        if not self.model:
            self.model = defaults.get("model", "")
        if not self.base_url:
            self.base_url = defaults.get("base_url", "")


class BaseProvider(ABC):
    @abstractmethod
    def create_llm(self, config: ProviderConfig) -> BaseChatModel:
        ...

    @abstractmethod
    def get_default_model(self) -> str:
        ...

    @abstractmethod
    def get_default_base_url(self) -> str:
        ...

    @abstractmethod
    def get_display_name(self) -> str:
        ...


class OpenAICompatibleProvider(BaseProvider):
    def create_llm(self, config: ProviderConfig) -> ChatOpenAI:
        # 隐式默认值: max_tokens=16384, timeout=300(5分钟)
        # 用户可在 setting.json 中通过 max_tokens/timeout 字段显式覆盖
        return ChatOpenAI(
            base_url=config.base_url,
            api_key=config.api_key,
            model=config.model,
            temperature=config.temperature,
            max_tokens=config.max_tokens if config.max_tokens is not None else 16384,
            timeout=config.timeout if config.timeout is not None else 300,
        )

    def get_default_model(self) -> str:
        return PROVIDER_DEFAULTS["openai_compatible"]["model"]

    def get_default_base_url(self) -> str:
        return PROVIDER_DEFAULTS["openai_compatible"]["base_url"]

    def get_display_name(self) -> str:
        return PROVIDER_DISPLAY_NAMES[ProviderType.OPENAI_COMPATIBLE]


class AnthropicProvider(BaseProvider):
    def create_llm(self, config: ProviderConfig) -> Any:
        if ChatAnthropic is None:
            raise ImportError(
                "使用 Anthropic 需要安装 langchain-anthropic:\n"
                "  pip install langchain-anthropic"
            )
        kwargs: dict[str, Any] = {
            "model": config.model,
            "temperature": config.temperature,
        }
        if config.api_key:
            kwargs["anthropic_api_key"] = config.api_key
        if config.base_url and config.base_url != PROVIDER_DEFAULTS["anthropic"]["base_url"]:
            kwargs["anthropic_api_url"] = config.base_url
        return ChatAnthropic(**kwargs)

    def get_default_model(self) -> str:
        return PROVIDER_DEFAULTS["anthropic"]["model"]

    def get_default_base_url(self) -> str:
        return PROVIDER_DEFAULTS["anthropic"]["base_url"]

    def get_display_name(self) -> str:
        return PROVIDER_DISPLAY_NAMES[ProviderType.ANTHROPIC]


PROVIDER_REGISTRY: dict[ProviderType, BaseProvider] = {
    ProviderType.OPENAI_COMPATIBLE: OpenAICompatibleProvider(),
    ProviderType.ANTHROPIC: AnthropicProvider(),
}


def create_llm(config: ProviderConfig) -> BaseChatModel:
    provider = PROVIDER_REGISTRY.get(config.provider_type)
    if provider is None:
        raise ValueError(f"不支持的 Provider: {config.provider_type}")
    return provider.create_llm(config)


def get_provider_display(config: ProviderConfig) -> str:
    name = PROVIDER_DISPLAY_NAMES.get(config.provider_type, config.provider_type.value)
    return f"{name} / {config.model}"


def parse_provider(value: str) -> ProviderType | None:
    try:
        return ProviderType(value)
    except ValueError:
        return ProviderType._missing_(value)


def get_available_providers() -> list[dict]:
    return [
        {
            "key": pt.value,
            "display": PROVIDER_DISPLAY_NAMES.get(pt, pt.value),
            "short": PROVIDER_SHORT_NAMES.get(pt, pt.value),
            "default_model": PROVIDER_DEFAULTS.get(pt.value, {}).get("model", ""),
            "default_base_url": PROVIDER_DEFAULTS.get(pt.value, {}).get("base_url", ""),
        }
        for pt in ProviderType
    ]