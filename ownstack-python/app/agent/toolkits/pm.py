"""Product Manager (PM) Agent Toolkit."""
from typing import Dict, Any, List, Callable, Awaitable

from app.agent.toolkits.base import Toolkit
from app.agent.providers import ToolDefinition, ToolCall

class PMToolkit(Toolkit):
    """Toolkit for Product Management operations (Specifications, Planning)."""
    
    def get_definitions(self) -> List[ToolDefinition]:
        return [
            ToolDefinition(
                name="create_specification",
                description="Create a detailed technical specification (implementation_plan.md)",
                parameters={
                    "type": "object",
                    "properties": {
                        "feature_name": {"type": "string", "description": "Name of the feature"},
                        "requirements": {"type": "string", "description": "Raw user requirements"},
                    },
                    "required": ["feature_name", "requirements"],
                },
            ),
            ToolDefinition(
                name="review_plan",
                description="Review an existing implementation plan for gaps",
                parameters={
                    "type": "object",
                    "properties": {
                        "plan_path": {"type": "string", "description": "Path to the plan file"},
                    },
                    "required": ["plan_path"],
                },
            ),
        ]

    def get_handlers(self) -> Dict[str, Callable[[ToolCall], Awaitable[Dict[str, Any]]]]:
        return {
            "create_specification": self.create_specification,
            "review_plan": self.review_plan,
        }

    async def create_specification(self, call: ToolCall) -> Dict[str, Any]:
        # Placeholder for real logic (writing to file, etc)
        feature = call.arguments["feature_name"]
        return {"content": f"Specification for {feature} created successfully."}

    async def review_plan(self, call: ToolCall) -> Dict[str, Any]:
        # Placeholder for real logic
        path = call.arguments["plan_path"]
        return {"content": f"Plan at {path} reviewed. No critical gaps found."}
