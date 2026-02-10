"""Engineering agent core loop with pluggable LLM providers."""
from __future__ import annotations

import asyncio
import json
import os
import re
import uuid
from dataclasses import dataclass, field
from typing import Any, AsyncGenerator, Callable, Dict, List, Optional

from pydantic import BaseModel

from app.agent.providers import (
    LLMProvider,
    LLMResponse,
    Message,
    Role,
    ToolCall,
    ToolDefinition,
    ProviderConfig,
    get_provider,
)
from app.core.policies import check_command
from app.agent.memory import RulesLoader
from app.agent.providers.context_manager import ContextManager
from app.agent.artifact_manager import ArtifactManager
from app.utils.telemetry import get_black_box
from app.tools.browser import get_secure_browser
from app.tools.git_time import get_time_machine  # Added for Pillar 3
from app.agent.prompts import (
    ENGINEER_PROMPT,
    QA_PROMPT,
    SECURITY_PROMPT,
    DOCS_PROMPT,
    DESIGNER_PROMPT,
    PM_PROMPT,
    REVIEWER_PROMPT,
    ORCHESTRATOR_PROMPT,
)



class AgentEvent(BaseModel):
    """Event emitted during agent execution."""
    event: str
    data: Dict[str, Any]


@dataclass
class AgentConfig:
    """Configuration for the agent."""
    provider: str = "openai"
    model: str = "gpt-4o"
    role: str = "engineer"
    api_key: Optional[str] = None
    base_url: Optional[str] = None
    max_steps: int = 10
    max_self_corrections: int = 3
    verify_command: Optional[str] = None
    verify_url: Optional[str] = None # New: Visual proof URL
    model_tiering: bool = True # New: Enable Architect/Worker switch
    temperature: float = 0.2
    # SOTA Phase 73 Features
    enable_reflection: bool = True  # Auto-critique after each tool call
    enable_parallel_tools: bool = True  # Execute independent tools in parallel
    context_window_warning: int = 100000  # Warn when context approaches this limit
    enable_structured_output: bool = False  # Force Pydantic validation on responses

    def __post_init__(self) -> None:
        """
        Validate configuration values after initialization.
        
        The provider and model fields are required and must be non-empty strings.
        The role must be one of the supported agent roles.
        Temperature must be between 0 and 1.
        """
        valid_roles = {
            "engineer", "qa", "security", "docs", 
            "designer", "pm", "reviewer", "orchestrator"
        }
        
        if not self.provider or not isinstance(self.provider, str):
            raise ValueError("Provider must be a non-empty string")
            
        if not self.model or not isinstance(self.model, str):
            raise ValueError("Model must be a non-empty string")
            
        if self.role not in valid_roles:
            raise ValueError(f"Invalid role '{self.role}'. Must be one of: {', '.join(valid_roles)}")
            
        if not isinstance(self.temperature, (int, float)) or not (0.0 <= self.temperature <= 1.0):
            raise ValueError(f"Temperature must be between 0.0 and 1.0, got {self.temperature}")
            
        if self.max_steps <= 0:
            raise ValueError(f"max_steps must be positive, got {self.max_steps}")
            
        if self.max_self_corrections < 0:
            raise ValueError(f"max_self_corrections must be non-negative, got {self.max_self_corrections}")


SYSTEM_PROMPT = """Tu es un ingénieur logiciel expert travaillant dans un environnement Docker sandboxé.

## Langue de communication
Tu dois TOUJOURS communiquer en FRANÇAIS. Tes explications et ton plan doivent être en français.

## Flux d'interaction
1. Commence TOUJOURS ta première réponse par un bloc 'PLAN:' concis décrivant ce que tu comptes faire (en français).
2. Après le plan, procède aux appels d'outils (lecture de fichiers, exécution de tests, etc.).
3. Si tu dois modifier ton plan, fournis un nouveau bloc 'PLAN:' mis à jour.

## Outils Disponibles
- read_file: Lire le contenu d'un fichier
- write_file: Écrire du contenu dans un fichier
- execute_command: Exécuter des commandes shell
- apply_patch: Appliquer un patch diff unifié
- lsp_rename: Renommer un symbole proprement via LSP (plus sûr qu'un replace textuel)
- lsp_definitions: Trouver la définition d'un symbole (retourne fichier, ligne, colonne)
- lsp_references: Trouver toutes les références d'un symbole
- browse_url: Naviguer sur une URL, cliquer ou taper du texte (Testing UI)

## Artifacts (NOUVEAU)
Tu peux générer des documents structurés (plans, listes de tâches, preuves de tests) en utilisant des tags XML. Ils seront extraits et sauvegardés automatiquement dans `.ownstack/artifacts/`.
Exemple :
<artifact type="implementation_plan">
# Plan
...
</artifact>

## Raisonnement Systématique (Phase 40)
1. **Planification** : Génère TOUJOURS un artefact <artifact type="PLAN"> avant d'écrire du code.
2. **Réflexion** : En cas d'échec, analyse l'erreur dans un bloc <artifact type="SCRATCHPAD" name="reflexion">.
3. **Trace de Travail** : Utilise <artifact type="TODO"> pour suivre tes sous-tâches.
4. **Preuve de Succès** : Résume tes vérifications dans <artifact type="PROOF">.
5. **Délégation** : Si une tâche nécessite une expertise spécifique (ex: sécurité), utilise `delegate_task`.

## Directives
1. Lis toujours les fichiers avant de les modifier.
2. Utilise execute_command pour lancer les tests après les modifications.
3. Répare toutes les erreurs avant de terminer.
4. Communique de façon concise en FRANÇAIS.

{rules}"""


@dataclass
class BaseAgent:
    """
    Autonomous engineering agent with pluggable LLM backend.
    
    Usage:
        config = AgentConfig(provider='\''openai'\'', model='\''gpt-4o'\'')
        agent = EngineeringAgent(session_id='\''...'\'', runtime=runtime, config=config)
        result = await agent.run("Fix the bug in main.py")
    """
    session_id: str
    runtime: Any  # RuntimeManager
    config: AgentConfig = field(default_factory=AgentConfig)
    container_id: Optional[str] = None
    rules_loader: Optional[RulesLoader] = None
    
    _provider: Optional[LLMProvider] = field(default=None, repr=False, init=False)
    _provider_cache: Dict[str, LLMProvider] = field(default_factory=dict, repr=False, init=False)
    mcp_clients: List[Any] = field(default_factory=list, repr=False, init=False)
    last_provider_response: Optional[LLMResponse] = field(default=None, repr=False, init=False)
    current_artifacts: List[Dict[str, Any]] = field(default_factory=list, repr=False, init=False)
    artifact_manager: Optional[ArtifactManager] = field(default=None, repr=False, init=False)
    toolkits: Dict[str, Any] = field(default_factory=dict, repr=False, init=False)
    toolkit_handlers: Dict[str, Callable] = field(default_factory=dict, repr=False, init=False)
    
    def __init__(
        self,
        session_id: str,
        runtime: Any,
        config: Optional[AgentConfig] = None,
        container_id: Optional[str] = None,
        rules_loader: Optional[RulesLoader] = None,
        **kwargs
    ) -> None:
        self.session_id = session_id
        self.runtime = runtime
        self.container_id = container_id
        self.rules_loader = rules_loader
        self._provider_cache = {}
        self.mcp_clients = []
        self.last_provider_response = None
        self._provider = None
        self.current_artifacts = []
        # Local artifact manager used for reasoning gate and lifecycle
        from app.agent.artifact_manager import ArtifactManager
        self.artifact_manager = ArtifactManager(self.runtime.settings.workspace_root)
        self.toolkits = {}
        self.toolkit_handlers = {}
        
        if config:
            self.config = config
        else:
            self.config = AgentConfig(**kwargs)
            
        self.__post_init__()

    def __post_init__(self) -> None:
        """Initialize the LLM provider and toolkits from the configuration."""
        self._provider = self._get_tiered_provider(self.config.model)
        self._init_mcp()
        self._setup_toolkits()

    def _setup_toolkits(self) -> None:
        """Initialize and register toolkits (SOTA Architecture)."""
        from app.agent.toolkits.core import CoreToolkit
        from app.agent.toolkits.lsp import LSPToolkit
        from app.agent.toolkits.extra import ExtraToolkit
        from app.agent.toolkits.mcp import MCPToolkit
        
        # Specialists
        from app.agent.toolkits.pm import PMToolkit
        from app.agent.toolkits.qa import QAToolkit
        from app.agent.toolkits.designer import DesignerToolkit
        from app.agent.toolkits.reviewer import ReviewerToolkit
        from app.agent.toolkits.security import SecurityToolkit
        from app.agent.toolkits.docs import DocsToolkit
        
        # Core toolkits
        self.toolkits["core"] = CoreToolkit(self.runtime, self.container_id)
        self.toolkits["lsp"] = LSPToolkit(self.runtime, self.container_id)
        self.toolkits["extra"] = ExtraToolkit(self.runtime, self.container_id, self.session_id, self.rules_loader, self.config)
        self.toolkits["mcp"] = MCPToolkit(self.runtime, self.container_id, self.mcp_clients)
        
        # Specialist toolkits
        self.toolkits["pm"] = PMToolkit(self.runtime, self.container_id)
        self.toolkits["qa"] = QAToolkit(self.runtime, self.container_id)
        self.toolkits["designer"] = DesignerToolkit(self.runtime, self.container_id)
        self.toolkits["reviewer"] = ReviewerToolkit(self.runtime, self.container_id)
        self.toolkits["security"] = SecurityToolkit(self.runtime, self.container_id)
        self.toolkits["docs"] = DocsToolkit(self.runtime, self.container_id)
        
        # Register handlers
        for tk in self.toolkits.values():
            self.toolkit_handlers.update(tk.get_handlers())

    def _init_mcp(self):
        """Initialize MCP clients from environment/config."""
        mcp_servers = os.getenv("MCP_SERVERS")
        if mcp_servers:
            try:
                configs = json.loads(mcp_servers)
                for name, cfg in configs.items():
                    # Simplified for refactoring
                    pass
            except Exception as e:
                print(f"ERROR: Failed to init MCP: {e}")
    
    def _get_tiered_provider(self, model: str) -> LLMProvider:
        """Get or create provider for a specific model tier."""
        if model in self._provider_cache:
            return self._provider_cache[model]
            
        provider_config = ProviderConfig(
            provider=self.config.provider,
            model=model,
            api_key=self.config.api_key or os.getenv(f"{self.config.provider.upper()}_API_KEY"),
            base_url=self.config.base_url,
        )
        p = get_provider(provider_config)
        self._provider_cache[model] = p
        return p

    # ========== SOTA Phase 73: Helper Methods ==========
    
    def _estimate_context_tokens(self, messages: List[Message]) -> int:
        """Estimate token count for context window monitoring."""
        # Rough estimate: 4 chars = 1 token (conservative)
        total_chars = sum(len(m.content or "") for m in messages)
        return total_chars // 4
    
    def _check_context_warning(self, messages: List[Message]) -> Optional[str]:
        """Check if context is approaching the warning threshold."""
        estimated = self._estimate_context_tokens(messages)
        if estimated > self.config.context_window_warning:
            pct = int((estimated / self.config.context_window_warning) * 100)
            return f"[CONTEXT WARNING] Approche de {pct}% du seuil ({estimated} tokens estimés)"
        return None
    
    async def _execute_tools_parallel(self, tool_calls: List[ToolCall]) -> List[Dict[str, Any]]:
        """
        Execute multiple tool calls in parallel when they are independent.
        SOTA Phase 73: Parallel Tool Execution.
        """
        # Identify dependencies (tools that modify files should be sequential)
        write_tools = {"write_file", "write_to_file", "apply_patch", "lsp_rename", 
                       "replace_file_content", "multi_replace_file_content"}
        
        read_tools = {"read_file", "view_file", "execute_command", "browse_url"}
        
        # Group: Independent reads can be parallelized, writes must be sequential
        parallel_batch = []
        sequential_batch = []
        
        for tc in tool_calls:
            if tc.name in write_tools:
                sequential_batch.append(tc)
            else:
                parallel_batch.append(tc)
        
        results = []
        
        # Execute parallel batch concurrently
        if parallel_batch and self.config.enable_parallel_tools:
            parallel_tasks = [self._execute_tool(tc) for tc in parallel_batch]
            parallel_results = await asyncio.gather(*parallel_tasks, return_exceptions=True)
            for tc, res in zip(parallel_batch, parallel_results):
                if isinstance(res, Exception):
                    results.append((tc, {"content": str(res), "error": "parallel_execution_failed"}))
                else:
                    results.append((tc, res))
        else:
            # Fallback to sequential for parallel batch too
            for tc in parallel_batch:
                results.append((tc, await self._execute_tool(tc)))
        
        # Execute sequential batch one by one
        for tc in sequential_batch:
            results.append((tc, await self._execute_tool(tc)))
        
        return results
    
    def _generate_reflection(self, tool_name: str, result: Dict[str, Any]) -> Optional[str]:
        """
        Generate a reflection message after tool execution.
        SOTA Phase 73: Reflection Loop.
        """
        if not self.config.enable_reflection:
            return None
            
        content = result.get("content", "")
        has_error = result.get("error")
        
        if has_error:
            return f"[RÉFLEXION AUTO] L'outil `{tool_name}` a échoué. Considères: 1) Le chemin existe-t-il? 2) Les permissions sont-elles suffisantes? 3) La syntaxe est-elle correcte?"
        
        # Success reflection for certain tools
        if tool_name == "execute_command":
            if "error" in content.lower() or "failed" in content.lower():
                return f"[RÉFLEXION AUTO] La commande a retourné des warnings/erreurs. Vérifie le output avant de continuer."
        
        return None

    def _get_model_for_step(self, step: int, messages: List[Message]) -> str:
        """Determine which model to use for the current step."""
        if not self.config.model_tiering:
            return self.config.model
        
        last_msg = messages[-1] if messages else None
        
        if step > 2 and last_msg and last_msg.role == Role.TOOL:
            if "openai" in self.config.provider:
                return "gpt-4o-mini"
            if "anthropic" in self.config.provider:
                return "claude-3-haiku-20240307"
            if "openrouter" in self.config.provider:
                return "anthropic/claude-3-haiku-20240307"
                
        return self.config.model
    
    def _get_base_tools(self) -> List[ToolDefinition]:
        """
        Get standard tools available from base toolkits (SOTA).
        """
        tools = []
        base_names = ["core", "lsp", "extra", "mcp"]
        for name in base_names:
            tk = self.toolkits.get(name)
            if tk:
                tk_tools = tk.get_definitions()
                if name == "extra":
                    tk_tools = [t for t in tk_tools if t.name != "delegate_task"]
                tools.extend(tk_tools)
        return tools

    def _get_tools(self) -> List[ToolDefinition]:
        """
        Get available tools for the agent based on its role.
        """
        tools = self._get_base_tools()
        
        # Add role-specific tools from specialist toolkits
        role_tk = self.toolkits.get(self.config.role)
        if role_tk:
            tools.extend(role_tk.get_definitions())
            
        # Add delegation if orchestrator
        if self.config.role == "orchestrator":
            extra_tk = self.toolkits.get("extra")
            if extra_tk:
                delegate_tool = next((t for t in extra_tk.get_definitions() if t.name == "delegate_task"), None)
                if delegate_tool:
                    tools.append(delegate_tool)
            
        # Add dynamic MCP tools
        for client in self.mcp_clients:
            try:
                # This is a bit tricky as it's async in a sync context
                # For now, we'll assume tools are fetched during setup or use a sync wrapper
                import anyio
                mcp_tools = anyio.from_thread.run_sync(asyncio.run, client.list_tools())
                for mt in mcp_tools:
                    tools.append(ToolDefinition(
                        name=f"mcp_{mt['name']}",
                        description=mt['description'],
                        parameters=mt['inputSchema']
                    ))
            except Exception as e:
                print(f"WARNING: Could not fetch tools from MCP server: {e}")
                
        return tools

    def _get_system_prompt_template(self) -> str:
        """
        Get the system prompt template based on agent'\''s role.
        
        Returns:
            str: The role-specific prompt template to use for the agent'\''s system message.
        """
        if self.config.role == "qa":
            return QA_PROMPT
        elif self.config.role == "security":
            return SECURITY_PROMPT
        elif self.config.role == "docs":
            return DOCS_PROMPT
        elif self.config.role == "designer":
            return DESIGNER_PROMPT
        elif self.config.role == "pm":
            return PM_PROMPT
        elif self.config.role == "reviewer":
            return REVIEWER_PROMPT
        elif self.config.role == "orchestrator":
            return ORCHESTRATOR_PROMPT
        return ENGINEER_PROMPT
    
    def _build_system_prompt(self) -> str:
        """
        Build complete system prompt including project rules.
        
        Combines the role-specific prompt template with any project rules
        loaded from the rules_loader.
        
        Returns:
            str: The complete system prompt to use for the agent.
        """
        rules = ""
        if self.rules_loader:
            project_rules = self.rules_loader.get_rules()
            if not project_rules.is_empty():
                rules = f"\n## Project Rules\n{project_rules.to_system_prompt()}"
        
        template = self._get_system_prompt_template()
        return template.format(rules=rules)
    
    async def run(self, instructions: str) -> str:
        """
        Run agent with given instructions and return final response.
        
        Args:
            instructions (str): The task instructions for the agent.
            
        Returns:
            str: The final response from the agent after task completion.
        """
        final = ""
        async for event in self.run_stream(instructions):
            if event.event == "complete":
                final = event.data.get("response", "")
        return final
    
    async def run_stream(self, instructions: str) -> AsyncGenerator[AgentEvent, None]:
        """
        Run agent with streaming events for progress monitoring.
        
        Args:
            instructions (str): The task instructions for the agent.
            
        Yields:
            AgentEvent: Events during agent execution including:
                - tool calls and their results
                - completion status
                - errors if they occur
        """
        yield AgentEvent(event="start", data={"session_id": self.session_id})
        
        # Build conversation
        system_prompt = self._build_system_prompt()
        print(f"DEBUG: System Prompt: {system_prompt}")
        messages: List[Message] = [
            Message(role=Role.SYSTEM, content=system_prompt),
            Message(role=Role.USER, content=instructions),
        ]
        
        tools = self._get_tools()
        self_corrections = 0
        
        # P1 & P3: Context Management & Telemetry
        ctx_manager = ContextManager(model=self.config.model)
        telemetry = get_black_box(self.session_id, self.runtime.settings.workspace_root)
        # Use existing artifact manager from state
        artifact_manager = self.artifact_manager
        
        for step in range(self.config.max_steps):
            print(f"DEBUG: Agent Step {step + 1}/{self.config.max_steps}")
            yield AgentEvent(event="step", data={"step": step + 1})
            
            # Phase 42: Model Tiering (Architect/Worker)
            current_model = self._get_model_for_step(step, messages)
            provider = self._get_tiered_provider(current_model)
            if provider.model != current_model:
                 print(f"DEBUG: Switching to model {current_model} for this step")

            # P1: Context Pruning (Safety first)
            messages = ctx_manager.prune_context(messages)
            
            # P3: Log Prompt
            telemetry.log("step.start", {"step": step + 1, "model": current_model, "message_count": len(messages)})

            # Call LLM
            try:
                print(f"DEBUG: Calling LLM ({current_model}) with {len(messages)} messages")
                response = await provider.chat(
                    messages=messages,
                    tools=tools,
                    temperature=self.config.temperature,
                )
                self.last_provider_response = response # Store for potential future use (e.g., self-reflection)
                
                # P3: Log Response
                telemetry.log("llm.response", {
                    "content": response.content,
                    "tool_calls": [tc.__dict__ for tc in response.tool_calls] if response.tool_calls else []
                })
                print(f"DEBUG: LLM Response content: '\''{response.content}'\''")
                print(f"DEBUG: LLM Tool calls: {response.tool_calls}")
            except Exception as e:
                print(f"DEBUG: LLM Error: {e}")
                # Resilience: try one retry if it's a transient API error
                if "429" in str(e) or "500" in str(e):
                    await asyncio.sleep(2)
                    continue
                yield AgentEvent(event="error", data={"error": str(e)})
                break
            
            # Phase 38: Artifact Extraction
            artifacts = artifact_manager.extract_artifacts(response.content)
            
            # Persistent Thinking Scaffold
            thinking_match = re.search(r'<thinking>(.*?)</thinking>', response.content, re.DOTALL)
            if thinking_match:
                thinking_content = thinking_match.group(1).strip()
                artifacts.append({
                    "type": "thinking",
                    "name": "scratchpad",
                    "content": thinking_content
                })

            if artifacts:
                print(f"DEBUG: Extracted {len(artifacts)} artifacts")
                await artifact_manager.save_artifacts(artifacts, self.runtime, self.container_id)
                yield AgentEvent(event="artifacts", data={"artifacts": artifacts})

            # PHASE 39: Reasoning Gate Enforcement
            # If agent tries to write code but hasn't generated a plan.md yet, block it.
            write_tools = ["write_to_file", "apply_patch", "replace_file_content", "multi_replace_file_content"]
            if any(tc.name in write_tools for tc in response.tool_calls):
                # Check if a plan artifact was generated in this session
                # We'll use a local flag or check the artifacts list
                plan_exists = any(a.get("name") == "plan" for a in artifact_manager.current_artifacts)
                if not plan_exists:
                    print("WARNING: Reasoning Gate Blocked code modification. Plan required.")
                    # Inject a system correction asking for a plan
                    messages.append(Message(
                        role=Role.SYSTEM,
                        content="ATTENTION: Vous essayez de modifier du code sans avoir généré d'artefact <artifact name='plan'>. Veuillez d'abord décomposer votre tâche complexe en un plan détaillé."
                    ))
                    continue # Re-run loop with correction

            # Handle tool calls
            if response.has_tool_calls:
                print(f"DEBUG: Handling {len(response.tool_calls)} tool calls")
                # Add assistant message with tool calls
                messages.append(Message(
                    role=Role.ASSISTANT,
                    content=response.content or "",
                    tool_calls=[
                        {
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": json.dumps(tc.arguments)
                            }
                        }
                        for tc in response.tool_calls
                    ],
                ))
                
                # Execute tools
                for tc in response.tool_calls:
                    # Pillar 3: Auto-snapshot before modification
                    checkpoint_id = None
                    if tc.name in ("write_file", "apply_patch", "lsp_rename") and self.container_id:
                        try:
                            tm = get_time_machine(self.runtime)
                            snapshot = await tm.snapshot(self.container_id, f"Pre-{tc.name} for {tc.id}")
                            checkpoint_id = snapshot.id
                        except Exception as e:
                            print(f"DEBUG: Auto-snapshot failed: {e}")

                    yield AgentEvent(event="tool_call", data={
                        "id": tc.id,
                        "name": tc.name,
                        "arguments": tc.arguments,
                        "checkpoint_id": checkpoint_id, # Sent to Extension
                    })
                    
                    result = await self._execute_tool(tc)
                    
                    yield AgentEvent(event="tool_result", data={
                        "id": tc.id,
                        "name": tc.name,
                        "success": "error" not in result,
                        "output": result.get("content", ""),
                    })
                    
                    # Add tool result message
                    # Phase 42: Succinct Feedback. 
                    # If output is > 5000 chars, we provide a summary to the history 
                    # (The full output is still available in logs/artifacts if needed, 
                    # and ContextManager handles the final pruning anyway).
                    tool_content = result.get("content", "")
                    if len(tool_content) > 5000:
                        tool_content = tool_content[:2500] + "\n... [LONG OUTPUT SUMMARIZED] ...\n" + tool_content[-2500:]

                    messages.append(Message(
                        role=Role.TOOL,
                        content=tool_content,
                        tool_call_id=tc.id,
                        name=tc.name,
                    ))
                    
                    # P3: Log Tool Action
                    telemetry.log("tool.result", {
                        "id": tc.id,
                        "name": tc.name,
                        "success": "error" not in result,
                        "output_summary": result.get("content", "")[:500] + "..." if len(result.get("content", "")) > 500 else result.get("content", "")
                    })
                    
                    if result.get("error"):
                        self_corrections += 1
                        # Phase 40: Reflexion Loop
                        # Add a reflection message to help the agent recover
                        messages.append(Message(
                            role=Role.SYSTEM,
                            content=f"[REFLEXION]: L'outil {tc.name} a échoué avec l'erreur: {result.get('error')}. Analyse la cause profonde (permissions, chemin inexistant, erreur de syntaxe) avant de réessayer."
                        ))
                        if self_corrections > self.config.max_self_corrections:
                                yield AgentEvent(event="max_corrections", data={
                                    "corrections": self_corrections
                                })
                                break
                
                # Verify if configured
                if self.config.verify_command and self.container_id:
                    stdout, stderr, code = await self.runtime.exec_capture_async(
                        self.container_id,
                        self.config.verify_command
                    )
                    yield AgentEvent(event="verify", data={
                        "command": self.config.verify_command,
                        "exit_code": code,
                        "stdout": stdout[:500],
                        "stderr": stderr[:500] if stderr else None,
                    })

                    # Phase 40: Systematic Visual Proof (Pillar 1)
                    if self.config.verify_url:
                        try:
                            browser = get_secure_browser()
                            url = self.config.verify_url
                            screenshot_result = await browser.browse(url)
                            
                            if "screenshot" in screenshot_result:
                                # Save as Proof Artifact
                                proof_id = f"proof_{uuid.uuid4().hex[:8]}"
                                self.artifact_manager.save_artifact(
                                    name=proof_id,
                                    type="proof",
                                    content=f"Visual Proof for {url}",
                                    metadata={
                                        "url": url,
                                        "title": screenshot_result.get("title"),
                                        "screenshot": screenshot_result["screenshot"]
                                    }
                                )
                                yield AgentEvent(event="visual_proof", data={
                                    "url": url,
                                    "title": screenshot_result.get("title"),
                                    "artifact_id": proof_id
                                })
                        except Exception as ve:
                            print(f"DEBUG: Visual Verification failed: {ve}")
                    
                    if code != 0:
                        # Phase 40: Reflexion Loop for Verification
                        messages.append(Message(
                            role=Role.SYSTEM,
                            content=f"[REFLEXION]: La vérification via '{self.config.verify_command}' a échoué (exit {code}).\nSTDOUT: {stdout}\nSTDERR: {stderr}\n\nPourquoi ton changement a-t-il échoué ? Propose une correction structurée."
                        ))
                        continue
            else:
                # Phase 39: Yield a "thinking" event to the extension
                thought = response.content or ""
                if "<thinking>" in thought:
                    # Extract content between <thinking> tags
                    match = re.search(r"<thinking>(.*?)</thinking>", thought, re.DOTALL)
                    if match:
                        thought = match.group(1).strip()
                
                yield AgentEvent(event="thinking", data={"thought": thought})

                # PHASE 39: Reasoning Gate Enforcement
                write_tools = ["write_to_file", "apply_patch", "replace_file_content", "multi_replace_file_content"]
                if any(tc.name in write_tools for tc in response.tool_calls):
                    plan_exists = any(a.get("name") == "plan" for a in self.artifact_manager.current_artifacts)
                    if not plan_exists:
                        messages.append(Message(
                            role=Role.SYSTEM,
                            content="ATTENTION: Vous essayez de modifier du code sans avoir généré d'artefact <artifact name='plan'>. Veuillez d'abord décomposer votre tâche complexe en un plan détaillé."
                        ))
                        continue

                # Phase 41: Actor-Evaluator (Reflexion) - Gated by ACI_ENABLED
                if os.environ.get("ACI_ENABLED") == "true":
                    eval_success, eval_feedback = await self._run_evaluation(response.content)
                    if not eval_success and step < self.config.max_steps - 1:
                        messages.append(Message(
                            role=Role.SYSTEM,
                            content=f"[EVALUATOR]: Ton travail n'est pas encore parfait. Feedback : {eval_feedback}\nAnalyse ce feedback et corrige ce qui manque."
                        ))
                        continue
                else:
                    eval_success, eval_feedback = True, "Evaluation skipped in legacy mode."

                yield AgentEvent(event="complete", data={
                    "response": response.content,
                    "steps": step + 1,
                    "evaluation": {"success": eval_success, "feedback": eval_feedback}
                })
                return
        
        # Only yield complete if we actually finished all steps without breaking
        if step == self.config.max_steps - 1:
            yield AgentEvent(event="complete", data={
                "response": "Reached maximum steps without completion.",
                "steps": self.config.max_steps
            })
    
    async def _execute_tool(self, call: ToolCall) -> Dict[str, Any]:
        """
        Execute a single tool call in the agent's container.
        Dispatches to modular toolkits (SOTA).
        """
        if not self.container_id:
            return {"content": "No container attached", "error": "no_container"}
        
        # SOTA: Dynamic Dispatch
        if call.name in self.toolkit_handlers:
            try:
                # Special case: Command Policy check (Security)
                if call.name == "execute_command" or call.name == "execute_network_command":
                    cmd = call.arguments.get("command") or call.arguments.get("cmd")
                    decision = check_command(cmd)
                    if decision == "DENY":
                        return {"content": f"Command DENIED by policy: {cmd}", "error": "policy_denied"}
                    elif decision == "ASK":
                        return {"content": f"Command requires approval: {cmd}", "error": "policy_approval_required"}

                return await self.toolkit_handlers[call.name](call)
            except Exception as e:
                import logging
                logging.exception(f"Toolkit execution failed: {call.name}")
                return {"content": f"Error executing tool {call.name}: {str(e)}", "error": "execution_failed"}

        # Special Case: MCP Dynamic Dispatch
        if call.name.startswith("mcp_"):
            mcp_tk = self.toolkits.get("mcp")
            if mcp_tk:
                return await mcp_tk.handle_mcp_call(call)

        return {"content": f"Unknown tool: {call.name}", "error": "unknown_tool"}

    async def _run_evaluation(self, result_text: str) -> Tuple[bool, str]:
        """
        Phase 41: Reflexion Evaluator.
        Grades the output and provides verbal reinforcement.
        """
        # For MVP: Lightweight evaluation prompt
        # In production: Use a specialized sub-agent with delegate_task
        eval_prompt = f"Tu es un Évaluateur de Code. Analyse le résultat suivant et dis si la tâche est accomplie à 100%.\nRésultat: {result_text}\n\nRéponds EXACTEMENT au format: [SUCCESS: True/False] Feedback: <tes observations>"
        
        # We reuse the provider's generate or a sub-agent if needed
        # For simplicity in the loop, we'll do a quick check here
        # (This is where Multivers A/B can compare different eval strategies)
        
        # If the result seems to indicate completion, allow it.
        # This is a placeholder for a real LLM call.
        if "Terminé" in result_text or "Succès" in result_text or "fix applied" in result_text.lower():
            return True, "Task completed based on indicators."
        
        return False, "Le résultat ne mentionne pas explicitement la fin de la tâche ou le succès des tests."


def create_agent(
    session_id: str,
    runtime: Any,
    container_id: str,
    provider: str = "openai",
    model: str = "gpt-4o",
    role: str = "engineer",
    **kwargs
) -> BaseAgent:
    """
    Factory function to create an agent.
    
    Args:
        session_id (str): Unique session identifier
        runtime (Any): Runtime manager instance
        container_id (str): Docker container ID
        provider (str, optional): LLM provider name. Defaults to "openai"
        model (str, optional): Model name. Defaults to "gpt-4o"
        role (str, optional): Agent role. Defaults to "engineer"
        **kwargs: Additional configuration options
        
    Returns:
        BaseAgent: Configured agent instance
    """
    config = AgentConfig(
        provider=provider,
        model=model,
        role=role,
        **kwargs
    )
    
    return BaseAgent(
        session_id=session_id,
        runtime=runtime,
        config=config,
        container_id=container_id,
    )

# Backward compatibility alias
EngineeringAgent = BaseAgent
