"""Anthropic Claude Provider."""
from __future__ import annotations

import json
import os
from typing import Any, AsyncIterator, Dict, List, Optional
import httpx

from .base import (
    LLMProvider, LLMResponse, Message, ToolCall, ToolDefinition,
    ProviderConfig, register_provider, Role
)


@register_provider("anthropic")
class AnthropicProvider(LLMProvider):
    """Anthropic Claude provider."""
    
    def __init__(self, config: ProviderConfig):
        self.config = config
        self.api_key = config.api_key or os.getenv("ANTHROPIC_API_KEY", "")
        self.base_url = config.base_url or "https://api.anthropic.com/v1"
        self.model = config.model
    
    @property
    def name(self) -> str:
        return "anthropic"
    
    async def chat(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
        max_tokens: Optional[int] = None,
    ) -> LLMResponse:
        """Send chat request to Anthropic."""
        payload = self._build_payload(messages, tools, temperature, max_tokens)
        
        data = await self._request("/messages", payload)
        return self._parse_response(data)
    
    async def stream(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
    ) -> AsyncIterator[str]:
        """Stream response from Anthropic using SSE."""
        payload = self._build_payload(messages, tools, temperature, None)
        payload["stream"] = True
        
        url = f"{self.base_url}/messages"
        headers = self._get_headers()
        
        async with httpx.AsyncClient(timeout=120.0) as client:
            async with client.stream("POST", url, json=payload, headers=headers) as response:
                if response.status_code != 200:
                    error_text = await response.aread()
                    raise RuntimeError(f"Anthropic API Error {response.status_code}: {error_text.decode('utf-8')}")

                async for line in response.aiter_lines():
                    line = line.strip()
                    if not line.startswith("data: "):
                        continue
                    
                    data_str = line[6:]
                    if data_str == "[DONE]":
                        break
                        
                    try:
                        event = json.loads(data_str)
                        if event["type"] == "content_block_delta":
                            delta = event.get("delta", {})
                            if delta.get("type") == "text_delta":
                                yield delta.get("text", "")
                        elif event["type"] == "error":
                            raise RuntimeError(f"Anthropic API Error: {event.get('error', {}).get('message')}")
                    except json.JSONDecodeError:
                        continue

    def _build_payload(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
        max_tokens: Optional[int] = None,
    ) -> Dict[str, Any]:
        """Build the API payload."""
        system = ""
        conversation = []
        for msg in messages:
            if msg.role == Role.SYSTEM:
                system = msg.content
            else:
                conversation.append({
                    "role": "user" if msg.role == Role.USER else "assistant",
                    "content": msg.content,
                })
        
        payload: Dict[str, Any] = {
            "model": self.model,
            "messages": conversation,
            "max_tokens": max_tokens or 4096,
            "temperature": temperature,
        }
        
        if system:
            # Phase 42: Prompt Caching (Anthropic)
            # Use the block format to specify cache_control
            payload["system"] = [
                {
                    "type": "text",
                    "text": system,
                    "cache_control": {"type": "ephemeral"}
                }
            ]
        
        if tools:
            payload["tools"] = self._format_tools(tools)
            
        return payload

    def _format_tools(self, tools: List[ToolDefinition]) -> List[Dict[str, Any]]:
        """Format tools for Anthropic API."""
        return [
            {
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            }
            for t in tools
        ]
    
    def _get_headers(self) -> Dict[str, str]:
        return {
            "Content-Type": "application/json",
            "x-api-key": self.api_key,
            "anthropic-version": "2023-06-01",
        }
    
    async def _request(self, endpoint: str, payload: Dict[str, Any]) -> Dict[str, Any]:
        """Make HTTP request to Anthropic API."""
        url = f"{self.base_url}{endpoint}"
        headers = self._get_headers()
        
        async with httpx.AsyncClient(timeout=120.0) as client:
            response = await client.post(url, json=payload, headers=headers)
            if response.status_code != 200:
                raise RuntimeError(f"Anthropic API Error {response.status_code}: {response.text}")
            return response.json()
    
    def _parse_response(self, data: Dict[str, Any]) -> LLMResponse:
        """Parse Anthropic API response."""
        content = ""
        tool_calls = []
        
        for block in data.get("content", []):
            if block["type"] == "text":
                content += block["text"]
            elif block["type"] == "tool_use":
                tool_calls.append(ToolCall(
                    id=block["id"],
                    name=block["name"],
                    arguments=block["input"],
                ))
        
        return LLMResponse(
            content=content or None,
            tool_calls=tool_calls,
            finish_reason=data.get("stop_reason", "stop"),
            usage=data.get("usage"),
        )
