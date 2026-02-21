import logging
import re
import json
from typing import List, Optional
from pydantic import BaseModel
from app.agent.providers.base import Message, Role
from app.core.globals import STATE

logger = logging.getLogger(__name__)

class ExecutionStep(BaseModel):
    id: int
    summary: str
    status: str = "pending"

class MissionPlan(BaseModel):
    steps: List[ExecutionStep]
    rationale: str

PLANNER_SYSTEM_PROMPT = """Tu es l'Architecte de OpenClaw. Ta mission est de décomposer une requête complexe en un plan d'action structuré.
Réponds TOUJOURS au format JSON suivant :
{
  "steps": [
    {"id": 1, "summary": "Action concise 1"},
    {"id": 2, "summary": "Action concise 2"}
  ],
  "rationale": "Explication brève de la stratégie"
}"""

class OpenClawPlanner:
    """The reasoning module. Inspired by OpenClaw/planner."""
    
    def __init__(self, provider):
        self.provider = provider

    async def generate_plan(self, task_description: str, spec: Optional[dict] = None, context: Optional[str] = None) -> MissionPlan:
        """
        Analyze the task and generate a structured mission plan.
        
        Args:
            task_description: User phrasing
            spec: dict representation of MissionSpec (objectives, scope, constraints)
            context: Additional context
        """
        constraints_str = ""
        if spec:
            constraints_str = f"""
CONTRACT CONSTRAINTS (MUST RESPECT):
- OBJECTIVES: {json.dumps(spec.get('objectives'))}
- SCOPE: {json.dumps(spec.get('scope'))}
- FORBIDDEN: {spec.get('stop_conditions')}
"""

        prompt = f"""Tâche : {task_description}

{constraints_str}

Contexte :
{context or 'Aucun contexte supplémentaire.'}
"""
        
        messages = [
            Message(role=Role.SYSTEM, content=PLANNER_SYSTEM_PROMPT),
            Message(role=Role.USER, content=prompt)
        ]
        
        response = await self.provider.chat(messages=messages)
        
        try:
            import json
            # Anti-Naïvety: Robust JSON extraction (handling text noise/markdown)
            content = response.content
            json_match = re.search(r'(\{.*\})', content, re.DOTALL)
            if json_match:
                content = json_match.group(1)
            
            data = json.loads(content)
            # Ensure required fields exist or provide defaults
            if "steps" not in data: data["steps"] = []
            if "rationale" not in data: data["rationale"] = "No rationale provided by LLM."
            
            return MissionPlan(**data)
        except Exception as e:
            logger.error(f"Failed to parse mission plan: {e}")
            # Fallback simple plan
            return MissionPlan(
                steps=[ExecutionStep(id=1, summary=task_description)],
                rationale="Erreur lors de la génération du plan détaillé, passage en mode direct."
            )
