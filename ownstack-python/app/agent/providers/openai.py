"""OpenAI and OpenAI-Compatible Provider.

Supports:
- OpenAI (GPT-4o, o1, etc.)
- Azure OpenAI
- Any OpenAI-compatible API (LM Studio, vLLM, etc.)
"""
from __future__ import annotations

import json
import os
from typing import Any, AsyncIterator, Dict, List, Optional
import httpx

from .base import (
    LLMProvider, LLMResponse, Message, ToolCall, ToolDefinition,
    ProviderConfig, register_provider, Role
)


@register_provider("openai")
@register_provider("openai-compatible")
class OpenAIProvider(LLMProvider):
    """OpenAI and compatible APIs provider."""
    
    def __init__(self, config: ProviderConfig):
        self.config = config
        self.api_key = config.api_key or os.getenv("OPENAI_API_KEY", "")
        self.base_url = config.base_url or "https://api.openai.com/v1"
        self.model = config.model
    
    @property
    def name(self) -> str:
        return "openai"
    
    async def chat(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
        max_tokens: Optional[int] = None,
    ) -> LLMResponse:
        """Send chat request to OpenAI."""
        payload: Dict[str, Any] = {
            "model": self.model,
            "messages": self._format_messages(messages),
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
        """Stream response from OpenAI using Server-Sent Events (SSE)."""
        payload = {
            "model": self.model,
            "messages": self._format_messages(messages),
            "temperature": temperature,
            "stream": True,
        }
        
        if tools:
            payload["tools"] = self._format_tools(tools)
        
        url = f"{self.base_url}/chat/completions"
        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self.api_key}",
        }

        async with httpx.AsyncClient(timeout=120.0) as client:
            async with client.stream("POST", url, json=payload, headers=headers) as response:
                if response.status_code != 200:
                    error_text = await response.aread()
                    raise RuntimeError(f"OpenAI API Error {response.status_code}: {error_text.decode('utf-8')}")

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
        """Format messages for OpenAI API."""
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
        """Format tools for OpenAI API."""
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
    
    async def _request(self, endpoint: str, payload: Dict[str, Any]) -> Dict[str, Any]:
        """Make HTTP request to OpenAI API using httpx."""
        url = f"{self.base_url}{endpoint}"
        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self.api_key}",
        }
        
        async with httpx.AsyncClient(timeout=120.0) as client:
            response = await client.post(url, json=payload, headers=headers)
            if response.status_code != 200:
                raise RuntimeError(f"OpenAI API Error {response.status_code}: {response.text}")
            return response.json()
    
    def _parse_response(self, data: Dict[str, Any]) -> LLMResponse:
        """Parse OpenAI API response."""
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
        )
