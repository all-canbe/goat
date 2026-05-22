from __future__ import annotations

from dataclasses import dataclass, field

from langchain_core.language_models.chat_models import BaseChatModel

from ..provider.provider import ProviderConfig, create_llm

DEFAULT_SUB_MODEL = "deepseek-chat"


@dataclass
class ModelRouterConfig:
    main_model: str = ""
    sub_model: str = DEFAULT_SUB_MODEL
    role_models: dict[str, str] = field(default_factory=dict)


class ModelRouter:
    def __init__(self, base_config: ProviderConfig, router_config: ModelRouterConfig):
        self._base_config = base_config
        self.router_config = router_config

    def get_llm_for_role(self, role: str) -> BaseChatModel:
        model = self.router_config.role_models.get(role, self.router_config.sub_model)
        config = ProviderConfig(
            provider_type=self._base_config.provider_type,
            api_key=self._base_config.api_key,
            model=model,
            base_url=self._base_config.base_url,
            temperature=self._base_config.temperature,
            max_tokens=self._base_config.max_tokens,
        )
        return create_llm(config)