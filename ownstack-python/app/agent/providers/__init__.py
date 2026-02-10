"""LLM Providers Package.

Pluggable LLM support for OwnStack.

Usage:
    from app.agent.providers import get_provider, ProviderConfig
    
    # OpenAI
    config = ProviderConfig(provider='openai', model='gpt-4o')
    provider = get_provider(config)
    
    # Anthropic
    config = ProviderConfig(provider='anthropic', model='claude-3-5-sonnet-20241022')
    provider = get_provider(config)
    
    # Ollama (local)
    config = ProviderConfig(provider='ollama', model='llama3.1:70b')
    provider = get_provider(config)
    
    # OpenRouter (100+ models with unified API)
    config = ProviderConfig(provider='openrouter', model='anthropic/claude-3.5-sonnet')
    provider = get_provider(config)
    
    # Any OpenAI-compatible API
    config = ProviderConfig(
        provider='openai-compatible',
        model='my-model',
        base_url='http://localhost:1234/v1',
        api_key='lm-studio'
    )
    provider = get_provider(config)
"""

from .base import (
    LLMProvider,
    LLMResponse,
    Message,
    Role,
    ToolCall,
    ToolDefinition,
    ProviderConfig,
    get_provider,
    list_providers,
)

# Import providers to register them
from . import openai
from . import anthropic
from . import ollama
from . import openrouter

__all__ = [
    "LLMProvider",
    "LLMResponse",
    "Message",
    "Role",
    "ToolCall",
    "ToolDefinition",
    "ProviderConfig",
    "get_provider",
    "list_providers",
]
