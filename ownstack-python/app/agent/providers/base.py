"""Abstract LLM Provider Interface.

Allows OwnStack to work with any LLM:
- OpenAI (GPT-4, o1, etc.)
- Anthropic (Claude)
- Ollama (local models)
- Any OpenAI-compatible API
"""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, AsyncIterator, Dict, List, Optional
from enum import Enum


class Role(str, Enum):
    """Message role."""
    SYSTEM = "system"
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"


@dataclass
class Message:
    """Chat message."""
    role: Role
    content: str
    name: Optional[str] = None
    tool_call_id: Optional[str] = None
    tool_calls: Optional[List[Dict[str, Any]]] = None


@dataclass
class ToolDefinition:
    """Tool/function definition for LLM."""
    name: str
    description: str
    parameters: Dict[str, Any]  # JSON Schema


@dataclass
class ToolCall:
    """Tool call from LLM."""
    id: str
    name: str
    arguments: Dict[str, Any]


@dataclass
class LLMResponse:
    """Response from LLM."""
    content: Optional[str] = None
    tool_calls: List[ToolCall] = field(default_factory=list)
    finish_reason: str = "stop"
    usage: Optional[Dict[str, int]] = None
    # SOTA Phase 73: Metrics
    latency_ms: Optional[float] = None  # Response time
    model_used: Optional[str] = None  # Actual model (may differ from requested)
    cached: bool = False  # Was this from cache?
    
    @property
    def has_tool_calls(self) -> bool:
        return len(self.tool_calls) > 0
    
    @property
    def estimated_cost(self) -> float:
        """Estimate cost based on usage (rough approximation)."""
        if not self.usage:
            return 0.0
        # GPT-4o pricing: ~$2.5/1M input, ~$10/1M output
        input_tokens = self.usage.get("prompt_tokens", 0)
        output_tokens = self.usage.get("completion_tokens", 0)
        return (input_tokens * 0.0000025) + (output_tokens * 0.00001)


class LLMProvider(ABC):
    """
    Abstract base class for LLM providers.
    
    Implement this to add support for any LLM.
    """
    
    @property
    @abstractmethod
    def name(self) -> str:
        """Provider name (e.g., 'openai', 'anthropic')."""
        pass
    
    @abstractmethod
    async def chat(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
        max_tokens: Optional[int] = None,
    ) -> LLMResponse:
        """
        Send messages and get response.
        
        Args:
            messages: Conversation history
            tools: Available tools/functions
            temperature: Creativity (0-1)
            max_tokens: Max response length
            
        Returns:
            LLMResponse with content or tool calls
        """
        pass
    
    @abstractmethod
    async def stream(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
    ) -> AsyncIterator[str]:
        """
        Stream response tokens.
        
        Yields:
            Response tokens as they arrive
        """
        pass
    
    def format_system_prompt(self, base_prompt: str, rules: Optional[str] = None) -> str:
        """Format system prompt with optional project rules."""
        if rules:
            return f"{base_prompt}\n\n## Project Rules\n{rules}"
        return base_prompt


@dataclass
class ProviderConfig:
    """Configuration for an LLM provider."""
    provider: str  # 'openai', 'anthropic', 'ollama', 'openai-compatible'
    model: str
    api_key: Optional[str] = None
    base_url: Optional[str] = None
    extra: Dict[str, Any] = field(default_factory=dict)
    # SOTA Phase 73: Resilience
    max_retries: int = 3  # Retry on transient errors
    retry_delay_base: float = 1.0  # Base delay in seconds (exponential backoff)
    circuit_breaker_threshold: int = 5  # Failures before opening circuit
    timeout_seconds: float = 120.0  # Request timeout


# Provider registry
_providers: Dict[str, type] = {}


def register_provider(name: str):
    """Decorator to register a provider class."""
    def decorator(cls: type):
        _providers[name] = cls
        return cls
    return decorator


def get_provider(config: ProviderConfig) -> LLMProvider:
    """
    Get provider instance from config.
    
    Usage:
        config = ProviderConfig(provider='openai', model='gpt-4o', api_key='...')
        provider = get_provider(config)
        response = await provider.chat(messages)
    """
    if config.provider not in _providers:
        raise ValueError(f"Unknown provider: {config.provider}. Available: {list(_providers.keys())}")
    
    return _providers[config.provider](config)


def list_providers() -> List[str]:
    """List available provider names."""
    return list(_providers.keys())
