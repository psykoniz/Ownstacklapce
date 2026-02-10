"""OpenRouter Provider.

Supports all models available on OpenRouter including:
- OpenAI (GPT-4o, o1, etc.)
- Anthropic (Claude)
- Meta (Llama)
- Google (Gemini)
- Mistral, Qwen, DeepSeek, and many more

OpenRouter provides a unified API for 100+ models with automatic
fallbacks and cost optimization.
"""
from __future__ import annotations

import os
import json
import httpx
from .context_manager import ContextManager
from .base import (
    LLMProvider, LLMResponse, Message, ToolCall, ToolDefinition,
    ProviderConfig, register_provider, Role
)

OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1"


@register_provider("openrouter")
class OpenRouterProvider(LLMProvider):
    """
    OpenRouter provider - unified API for 100+ LLM models.
    """
    
    def __init__(self, config: ProviderConfig):
        import os
        self.config = config
        self.api_key = config.api_key or os.getenv("OPENROUTER_API_KEY", "")
        self.base_url = config.base_url or OPENROUTER_BASE_URL
        self.model = config.model
        
        # OpenRouter specific headers
        self.site_url = config.extra.get("site_url") or os.getenv("OPENROUTER_SITE_URL", "https://ownstack.ai")
        self.app_name = config.extra.get("app_name") or os.getenv("OPENROUTER_APP_NAME", "OwnStack IDE")
        
        # Initialize Context Manager
        self.context_limit = self._get_model_context_limit(self.model)
        self.context_manager = ContextManager(model=self.model, max_tokens=self.context_limit)
    
    def _get_model_context_limit(self, model: str) -> int:
        """Hardcoded limits for common models (safeguard)."""
        if "claude-3-opus" in model or "claude-3.5-sonnet" in model:
            return 200000
        if "gpt-4" in model:
            return 128000
        if "gemini-1.5" in model:
            return 1000000 # 1M context
        return 32000 # Conservative default

    @property
    def name(self) -> str:
        return "openrouter"
    
    async def chat(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
        max_tokens: Optional[int] = None,
    ) -> LLMResponse:
        """Send chat request to OpenRouter."""
        
        # PRUNE CONTEXT
        pruned_messages = self.context_manager.prune_context(messages)
        
        payload: Dict[str, Any] = {
            "model": self.model,
            "messages": self._format_messages(pruned_messages),
            "temperature": temperature,
        }
        
        if max_tokens:
            payload["max_tokens"] = max_tokens
        
        if tools:
            payload["tools"] = self._format_tools(tools)
        
        data = await self._request("/chat/completions", payload)
        return self._parse_response(data)
    
    async def stream(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
    ) -> AsyncIterator[str]:
        """Stream response from OpenRouter using Server-Sent Events (SSE)."""
        
        # PRUNE CONTEXT
        pruned_messages = self.context_manager.prune_context(messages)
        
        payload = {
            "model": self.model,
            "messages": self._format_messages(pruned_messages),
            "temperature": temperature,
            "stream": True,
        }
        
        if tools:
            payload["tools"] = self._format_tools(tools)
        
        url = f"{self.base_url}/chat/completions"
        headers = self._get_headers()

        async with httpx.AsyncClient(timeout=180.0) as client:
            async with client.stream("POST", url, json=payload, headers=headers) as response:
                if response.status_code != 200:
                    error_text = await response.aread()
                    raise RuntimeError(f"OpenRouter API Error {response.status_code}: {error_text.decode('utf-8')}")

                async for line in response.aiter_lines():
                    line = line.strip()
                    if not line.startswith("data: "):
                        continue
                    
                    data = line[6:]
                    if data == "[DONE]":
                        break
                    
                    try:
                        chunk = json.loads(data)
                        if "choices" in chunk and chunk["choices"]:
                            delta = chunk["choices"][0].get("delta", {})
                            content = delta.get("content", "")
                            if content:
                                yield content
                    except json.JSONDecodeError:
                        continue
    
    def _format_messages(self, messages: List[Message]) -> List[Dict[str, Any]]:
        """Format messages for OpenRouter API."""
        result = []
        for msg in messages:
            m: Dict[str, Any] = {
                "role": msg.role.value,
                "content": msg.content,
            }
            if msg.name:
                m["name"] = msg.name
            if msg.tool_call_id:
                m["tool_call_id"] = msg.tool_call_id
            if msg.tool_calls:
                m["tool_calls"] = msg.tool_calls
            result.append(m)
        return result
    
    def _format_tools(self, tools: List[ToolDefinition]) -> List[Dict[str, Any]]:
        """Format tools for OpenRouter API."""
        return [
            {
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            }
            for t in tools
        ]
    
    def _get_headers(self) -> Dict[str, str]:
        return {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self.api_key}",
            "HTTP-Referer": self.site_url,
            "X-Title": self.app_name,
        }
    
    async def _request(self, endpoint: str, payload: Dict[str, Any]) -> Dict[str, Any]:
        """
        Make HTTP request to OpenRouter API with SOTA resilience.
        Phase 73: Retry with exponential backoff.
        """
        import time
        import random
        
        url = f"{self.base_url}{endpoint}"
        headers = self._get_headers()
        
        max_retries = getattr(self.config, 'max_retries', 3)
        retry_delay = getattr(self.config, 'retry_delay_base', 1.0)
        timeout = getattr(self.config, 'timeout_seconds', 120.0)
        
        last_error = None
        start_time = time.time()
        
        for attempt in range(max_retries + 1):
            try:
                async with httpx.AsyncClient(timeout=timeout) as client:
                    response = await client.post(url, json=payload, headers=headers)
                    
                    # Handle rate limiting (429) with exponential backoff
                    if response.status_code == 429:
                        if attempt < max_retries:
                            wait_time = retry_delay * (2 ** attempt) + random.uniform(0, 1)
                            print(f"[SOTA] Rate limited, waiting {wait_time:.1f}s before retry {attempt + 1}/{max_retries}")
                            await asyncio.sleep(wait_time)
                            continue
                    
                    # Handle server errors (5xx) with retry
                    if response.status_code >= 500:
                        if attempt < max_retries:
                            wait_time = retry_delay * (2 ** attempt)
                            print(f"[SOTA] Server error {response.status_code}, retrying in {wait_time:.1f}s")
                            await asyncio.sleep(wait_time)
                            continue
                    
                    if response.status_code != 200:
                        raise RuntimeError(f"OpenRouter API Error {response.status_code}: {response.text}")
                    
                    result = response.json()
                    # Track latency for metrics
                    result["_latency_ms"] = (time.time() - start_time) * 1000
                    return result
                    
            except httpx.TimeoutException as e:
                last_error = e
                if attempt < max_retries:
                    print(f"[SOTA] Timeout, retrying ({attempt + 1}/{max_retries})")
                    await asyncio.sleep(retry_delay)
                    continue
            except httpx.ConnectError as e:
                last_error = e
                if attempt < max_retries:
                    print(f"[SOTA] Connection error, retrying ({attempt + 1}/{max_retries})")
                    await asyncio.sleep(retry_delay)
                    continue
        
        raise RuntimeError(f"OpenRouter request failed after {max_retries} retries: {last_error}")
    
    def _parse_response(self, data: Dict[str, Any]) -> LLMResponse:
        """Parse OpenRouter API response with SOTA metrics."""
        if "error" in data:
             raise RuntimeError(f"OpenRouter API Error: {data['error']}")
             
        choice = data["choices"][0]
        message = choice["message"]
        
        tool_calls = []
        if "tool_calls" in message:
            for tc in message["tool_calls"]:
                tool_calls.append(ToolCall(
                    id=tc["id"],
                    name=tc["function"]["name"],
                    arguments=json.loads(tc["function"]["arguments"]),
                ))
        
        return LLMResponse(
            content=message.get("content"),
            tool_calls=tool_calls,
            finish_reason=choice.get("finish_reason", "stop"),
            usage=data.get("usage"),
            # SOTA Phase 73: Metrics
            latency_ms=data.get("_latency_ms"),
            model_used=data.get("model", self.model),
        )
