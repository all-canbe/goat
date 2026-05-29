from __future__ import annotations

from dataclasses import dataclass

from langchain_core.language_models.chat_models import BaseChatModel

from ..provider.provider import ProviderConfig, create_llm

DEFAULT_SUB_MODEL = "deepseek-chat"


@dataclass
class ModelRouterConfig:
    main_model: str = ""
    sub_model: str = DEFAULT_SUB_MODEL
    review_model: str = ""
    review_provider: ProviderConfig | None = None


class ModelRouter:
    def __init__(self, base_config: ProviderConfig, router_config: ModelRouterConfig):
        self._base_config = base_config
        self.router_config = router_config

    def get_llm_for_role(self, role: str) -> BaseChatModel:
        if role == "review":
            if self.router_config.review_provider:
                return create_llm(self.router_config.review_provider)
            model = self.router_config.review_model or self.router_config.main_model
        else:
            model = self.router_config.sub_model or self.router_config.main_model

        config = ProviderConfig(
            provider_type=self._base_config.provider_type,
            api_key=self._base_config.api_key,
            model=model,
            base_url=self._base_config.base_url,
            temperature=self._base_config.temperature,
            max_tokens=self._base_config.max_tokens,
        )
        return create_llm(config)