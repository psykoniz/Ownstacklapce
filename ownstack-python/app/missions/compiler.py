import logging
import json
import os
import subprocess
from typing import Optional, Dict
from app.agent.providers.base import Message, Role
from .models import MissionSpec, MissionMode, Permission, ExecutionStrategy

logger = logging.getLogger(__name__)

COMPILER_SYSTEM_PROMPT = """You are the OwnStack Mission Compiler.
Your goal is to transform a natural language user request into a rigorous technical contract (MissionSpec).

STRICT COMPILATION RULES:
1. MODE SELECTION:
   - 'A1_static_read': Pure file reading, grep, and parsing. No tool execution. Use for initial audits.
   - 'A2_safe_tooling': Allows non-destructive tools like LSP or linters.
   - 'B_dynamic_exec': Execution of tests or code in an ephemeral sandbox. This is the standard for most SOTA missions.
   - 'C_hypothetical': Planning only.
   
MISSION ARCHITECTURE:
- Prefer 'ownstack_agent' (NativeAgentBridge) for full control and security.
- Use 'claude_code' only if specific CLI features are requested.

2. STRATEGY: For 'B_dynamic_exec', always specify 'ephemeral_branch' or 'patch_log' to ensure safety.

3. ORACLES: Define technical truth sources (e.g., "pytest", "npm run compile").

4. STOP CONDITIONS: Be explicit (e.g., "failed_tests", "unresolved_imports").

RESPONSE FORMAT:
You MUST return a valid JSON object matching the MissionSpec model.
Example:
{
  "mode": "A1_static_read",
  "strategy": "dry_run",
  "objectives": ["Identify dead code in VS Code extension"],
  "scope": ["vscode-extension/src"],
  "permissions": ["fs_read"],
  "oracles": ["grep", "tree-sitter"],
  "stop_conditions": ["file_not_found"],
  "output_format": ["markdown_matrix"],
  "budget_tokens": 50000,
  "preflight_checks": {"pytest": true, "playwright": false}
}"""

class MissionCompiler:
    """The 'Prompt to Contract' translator. Refined for OwnStack-grade rigor."""
    
    def __init__(self, provider, workspace_root: str):
        self.provider = provider
        self.workspace_root = workspace_root

    def _run_preflight(self) -> Dict[str, bool]:
        """Detect available infrastructure and oracles."""
        checks = {
            "pytest": False,
            "npm": False,
            "playwright": False,
            "xvfb": False,
            "typescript": False
        }
        
        # Simple check for command existence
        for cmd in checks.keys():
            try:
                # Check for command on host (or stable runtime)
                subprocess.run([cmd, "--version"], capture_output=True, shell=True)
                checks[cmd] = True
            except:
                pass
        
        # Specific check for VS Code extension scripts
        pkg_json = os.path.join(self.workspace_root, "vscode-extension", "package.json")
        if os.path.exists(pkg_json):
            checks["vscode_extension_detected"] = True
            
        return checks

    async def compile_prompt(self, user_prompt: str, context: Optional[str] = None) -> MissionSpec:
        """Compile natural language into a technical MissionSpec."""
        preflight = self._run_preflight()
        
        input_text = f"USER REQUEST: {user_prompt}\n\nPREFLIGHT DATA (Available tools): {json.dumps(preflight)}\n\nCONTEXT:\n{context or 'No additional context.'}"
        
        messages = [
            Message(role=Role.SYSTEM, content=COMPILER_SYSTEM_PROMPT),
            Message(role=Role.USER, content=input_text)
        ]
        
        response = await self.provider.chat(messages=messages)
        
        try:
            content = response.content
            if "```json" in content:
                content = content.split("```json")[1].split("```")[0].strip()
            elif "```" in content:
                content = content.split("```")[1].split("```")[0].strip()
            
            data = json.loads(content)
            # Inject preflight checks into spec
            data["preflight_checks"] = preflight
            return MissionSpec(**data)
        except Exception as e:
            logger.error(f"Compilation failed: {e}")
            return MissionSpec(
                mode=MissionMode.A1_STATIC_READ,
                objectives=["Emergency fallback mission due to compiler failure"],
                scope=["."],
                permissions=[Permission.FS_READ],
                oracles=[],
                stop_conditions=["manual_review"],
                output_format=["error_log"],
                preflight_checks=preflight
            )
