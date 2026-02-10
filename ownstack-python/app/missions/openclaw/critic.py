import logging
from typing import Optional
from pydantic import BaseModel
from app.agent.providers.base import Message, Role

logger = logging.getLogger(__name__)

class Critique(BaseModel):
    analysis: str
    proposed_fix: str
    should_retry: bool

CRITIC_SYSTEM_PROMPT = """Tu es le Critique de OpenClaw. Ta tâche est d'analyser un échec d'exécution et de proposer une solution corrective.
Fais preuve d'une rigueur extrême : identifie si l'erreur vient d'un bug de logique, d'une dépendance manquante ou d'un test mal écrit.
Réponds TOUJOURS au format JSON :
{
  "analysis": "Explication technique de l'échec",
  "proposed_fix": "Description de l'action corrective à tenter",
  "should_retry": true
}"""

class OpenClawCritic:
    """The self-reflection engine. Inspired by OpenClaw/critic."""
    
    def __init__(self, provider):
        self.provider = provider

    async def analyze_failure(self, task: str, error_details: str) -> Critique:
        """Provide a technical critique of why a step failed."""
        prompt = f"Tâche échouée : {task}\n\nDétails de l'erreur :\n{error_details}"
        
        messages = [
            Message(role=Role.SYSTEM, content=CRITIC_SYSTEM_PROMPT),
            Message(role=Role.USER, content=prompt)
        ]
        
        response = await self.provider.chat(messages=messages)
        
        try:
            import json
            import re
            # Anti-Naïvety: Robust JSON extraction
            content = response.content
            json_match = re.search(r'(\{.*\})', content, re.DOTALL)
            if json_match:
                content = json_match.group(1)
            
            data = json.loads(content)
            return Critique(**data)
        except Exception as e:
            logger.error(f"Critic failed to generate critique: {e}")
            return Critique(
                analysis=f"Échec de l'analyse automatique (Erreur: {str(e)[:50]})",
                proposed_fix="Tenter une réexécution simple ou demander l'aide de l'humain.",
                should_retry=False
            )
