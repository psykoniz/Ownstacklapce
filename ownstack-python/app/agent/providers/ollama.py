"""Ollama Provider - Local LLM Support."""
from __future__ import annotations

import json
import os
from typing import Any, AsyncIterator, Dict, List, Optional
import httpx

from .base import (
    LLMProvider, LLMResponse, Message, ToolCall, ToolDefinition,
    ProviderConfig, register_provider, Role
)


@register_provider("ollama")
class OllamaProvider(LLMProvider):
    """
    Ollama provider for local LLMs.
    """
    
    def __init__(self, config: ProviderConfig):
        self.config = config
        self.base_url = config.base_url or os.getenv("OLLAMA_HOST", "http://localhost:11434")
        self.model = config.model
    
    @property
    def name(self) -> str:
        return "ollama"
    
    async def chat(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
        max_tokens: Optional[int] = None,
    ) -> LLMResponse:
        """Send chat request to Ollama."""
        payload = self._build_payload(messages, tools, temperature, max_tokens, stream=False)
        
        data = await self._request("/api/chat", payload)
        return self._parse_response(data)
    
    async def stream(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]] = None,
        temperature: float = 0.7,
    ) -> AsyncIterator[str]:
        """Stream response from Ollama (NDJSON)."""
        payload = self._build_payload(messages, tools, temperature, None, stream=True)
        
        url = f"{self.base_url}/api/chat"
        
        async with httpx.AsyncClient(timeout=300.0) as client:
            async with client.stream("POST", url, json=payload, headers={"Content-Type": "application/json"}) as response:
                if response.status_code != 200:
                    error_text = await response.aread()
                    raise RuntimeError(f"Ollama API Error {response.status_code}: {error_text.decode('utf-8')}")

                async for line in response.aiter_lines():
                    if not line:
                        continue
                    try:
                        chunk = json.loads(line)
                        if chunk.get("done", False):
                            break
                        
                        message = chunk.get("message", {})
                        content = message.get("content", "")
                        if content:
                            yield content
                    except json.JSONDecodeError:
                        continue

    def _build_payload(
        self,
        messages: List[Message],
        tools: Optional[List[ToolDefinition]],
        temperature: float,
        max_tokens: Optional[int],
        stream: bool
    ) -> Dict[str, Any]:
        """Build Ollama payload."""
        payload: Dict[str, Any] = {
            "model": self.model,
            "messages": self._format_messages(messages),
            "stream": stream,
            "options": {
                "temperature": temperature,
            }
        }
        
        if max_tokens:
            payload["options"]["num_predict"] = max_tokens
        
        if tools:
            payload["tools"] = self._format_tools(tools)
            
        return payload

    def _format_messages(self, messages: List[Message]) -> List[Dict[str, Any]]:
        """Format messages for Ollama API."""
        return [
            {
                "role": msg.role.value,
                "content": msg.content,
            }
            for msg in messages
        ]
    
    def _format_tools(self, tools: List[ToolDefinition]) -> List[Dict[str, Any]]:
        """Format tools for Ollama (OpenAI-compatible format)."""
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
        """Make HTTP request to Ollama."""
        url = f"{self.base_url}{endpoint}"
        
        async with httpx.AsyncClient(timeout=300.0) as client:
            response = await client.post(url, json=payload, headers={"Content-Type": "application/json"})
            if response.status_code != 200:
                raise RuntimeError(f"Ollama API Error {response.status_code}: {response.text}")
            return response.json()
    
    def _parse_response(self, data: Dict[str, Any]) -> LLMResponse:
        """Parse Ollama API response."""
        message = data.get("message", {})
        
        tool_calls = []
        if "tool_calls" in message:
            for tc in message["tool_calls"]:
                tool_calls.append(ToolCall(
                    id=tc.get("id", ""),
                    name=tc["function"]["name"],
                    arguments=tc["function"]["arguments"] if isinstance(tc["function"]["arguments"], dict) else json.loads(tc["function"]["arguments"]),
                ))
        
        return LLMResponse(
            content=message.get("content"),
            tool_calls=tool_calls,
            finish_reason="stop",
        )
