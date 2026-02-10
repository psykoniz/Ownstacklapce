"""Toolkit for extra operations (Browser, Delegation)."""
from typing import Dict, Any, List, Callable, Awaitable, Optional

from app.agent.toolkits.base import Toolkit
from app.agent.providers import ToolDefinition, ToolCall, Role, Message, ProviderConfig, get_provider

class ExtraToolkit(Toolkit):
    """Toolkit for browsing URLs and delegating tasks to other agents."""
    
    def __init__(self, runtime: Any, container_id: str, session_id: str, rules_loader: Any, config: Any):
        super().__init__(runtime, container_id)
        self.session_id = session_id
        self.rules_loader = rules_loader
        self.config = config

    def get_definitions(self) -> List[ToolDefinition]:
        tools = [
            ToolDefinition(
                name="browse_url",
                description="Naviguer sur une URL et interagir avec la page (Testing UI)",
                parameters={
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "L'URL à consulter"},
                        "action": {"type": "string", "enum": ["navigate", "click", "type", "screenshot"], "default": "navigate"},
                        "selector": {"type": "string", "description": "Sélecteur CSS pour click/type"},
                        "text": {"type": "string", "description": "Texte à taper"},
                    },
                    "required": ["url"],
                },
            ),
        ]
        
        # Only orchestrator role should have delegation (handled in core.py _get_tools filter for now)
        # But for SOTA, we can expose it if needed
        tools.append(ToolDefinition(
                name="delegate_task",
                description="Déléguer une tâche à un agent spécialiste",
                parameters={
                    "type": "object",
                    "properties": {
                        "role": {
                            "type": "string", 
                            "description": "Rôle cible",
                            "enum": ["pm", "engineer", "designer", "reviewer", "qa", "security", "docs"]
                        },
                        "instructions": {"type": "string", "description": "Instructions détaillées"},
                    },
                    "required": ["role", "instructions"],
                },
            ))
        return tools

    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        return {
            "browse_url": self.browse_url,
            "delegate_task": self.delegate_task,
        }

    async def browse_url(self, call: ToolCall) -> Dict[str, Any]:
        from app.tools.browser import get_secure_browser
        browser = get_secure_browser()
        url = call.arguments["url"]
        action = call.arguments.get("action", "navigate")
        selector = call.arguments.get("selector")
        text = call.arguments.get("text")
        
        result = await browser.browse(url, action=action, selector=selector, text=text)
        
        if "error" in result:
            return {"content": f"Browser Error: {result['error']}", "error": result["error"]}
        
        content = f"Page: {result['title']}\nURL: {result['url']}\n\nContent:\n{result['text_content'][:2000]}..."
        if result.get("semantic_elements"):
            content += "\n\nSemantic Elements:\n" + str(result["semantic_elements"])
        
        return {"content": content, "screenshot": result.get("screenshot")}

    async def delegate_task(self, call: ToolCall) -> Dict[str, Any]:
        role = call.arguments["role"]
        instructions = call.arguments["instructions"]
        
        from app.agent.core import BaseAgent, AgentConfig
        sub_config = AgentConfig(
            provider=self.config.provider,
            model=self.config.model,
            role=role,
            api_key=self.config.api_key,
            base_url=self.config.base_url,
            max_steps=self.config.max_steps,
            temperature=self.config.temperature
        )
        
        sub_agent = BaseAgent(
            session_id=self.session_id,
            runtime=self.runtime,
            container_id=self.container_id,
            config=sub_config,
            rules_loader=self.rules_loader
        )
        
        result_text = await sub_agent.run(instructions)
        return {"content": f"[{role.upper()}] Report:\n{result_text}"}
