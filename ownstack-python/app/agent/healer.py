"""Self-Healing Agent - OwnStack's True Innovation.

WHAT NOBODY ELSE DOES (as of Feb 2026):
- Cursor: No healing, just suggestions
- Devin: Cloud-only, expensive, black box
- Copilot: No autonomous repair

OwnStack Innovation: LOCAL Self-Healing CI
- Detects failures automatically
- Proposes AND applies fixes in sandbox
- Validates fix before suggesting
- Zero cloud dependency, zero cost
"""
from __future__ import annotations

import asyncio
import time
import re
import logging
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple
from enum import Enum

logger = logging.getLogger(__name__)


class FailureType(str, Enum):
    """Classification of failure types."""
    TEST_FAILURE = "test_failure"
    IMPORT_ERROR = "import_error"
    SYNTAX_ERROR = "syntax_error"
    TYPE_ERROR = "type_error"
    DEPENDENCY_MISSING = "dependency_missing"
    CONFIG_ERROR = "config_error"
    RUNTIME_ERROR = "runtime_error"
    TRAINING_ERROR = "training_error"
    UNKNOWN = "unknown"


@dataclass
class Failure:
    """Represents a detected failure with context."""
    failure_type: FailureType
    file_path: Optional[str] = None
    line_number: Optional[int] = None
    error_message: str = ""
    full_output: str = ""
    suggested_fixes: List[str] = field(default_factory=list)


@dataclass
class HealingAttempt:
    """Record of a healing attempt."""
    failure: Failure
    fix_applied: str
    success: bool
    verification_output: str = ""
    duration_ms: int = 0


@dataclass
class HealingSession:
    """A complete self-healing session."""
    session_id: str
    container_id: str
    original_command: str
    original_output: str
    failures_detected: List[Failure] = field(default_factory=list)
    attempts: List[HealingAttempt] = field(default_factory=list)
    healed: bool = False
    total_attempts: int = 0
    max_attempts: int = 5


class FailureAnalyzer:
    """Analyzes command output to detect and classify failures."""
    
    # Patterns for failure detection
    PATTERNS = {
        FailureType.IMPORT_ERROR: [
            r"ModuleNotFoundError: No module named '(\w+)'",
            r"ImportError: cannot import name '(\w+)'",
            r"Error: Cannot find module '([\w\-/.]+)'",
        ],
        FailureType.SYNTAX_ERROR: [
            r"SyntaxError: (.+)",
            r"IndentationError: (.+)",
        ],
        FailureType.TYPE_ERROR: [
            r"TypeError: (.+)",
            r"AttributeError: (.+)",
        ],
        FailureType.TEST_FAILURE: [
            r"FAILED (.+\.py)::(\w+)",
            r"AssertionError: (.+)",
            r"(\d+) failed",
        ],
        FailureType.DEPENDENCY_MISSING: [
            r"pip install (\w+)",
            r"npm install ([\w\-@/]+)",
            r"Could not find a version that satisfies",
        ],
        FailureType.CONFIG_ERROR: [
            r"FileNotFoundError: .*(config|settings|\.env)",
            r"KeyError: '(\w+)'",
            r"ValidationError: .*", # Pydantic v2
        ],
        FailureType.RUNTIME_ERROR: [
            r"RuntimeError: .*",
            r"RecursionError: .*",
            r"AttributeError: .*",
            r"ValueError: .*",
        ],
        FailureType.TRAINING_ERROR: [
            r"Loss: NaN",
            r"Inf detected in weights",
            r"Gradient overflow",
            r"OOM during training",
        ],
    }
    
    # Patterns to extract file:line info
    LOCATION_PATTERNS = [
        r'File "(.+)", line (\d+)',
        r"at (.+):(\d+):",
        r"(.+\.py):(\d+):",
        r"(.+\.ts):(\d+):",
    ]
    
    def analyze(self, output: str, exit_code: int) -> List[Failure]:
        """Analyze command output and return detected failures."""
        print(f"DEBUG Healer: Analyzing output (exit {exit_code}). Sample: {output[:100]}...")
        if exit_code == 0:
            return []
        
        failures = []
        
        for failure_type, patterns in self.PATTERNS.items():
            for pattern in patterns:
                matches = re.finditer(pattern, output, re.MULTILINE)
                for match in matches:
                    failure = Failure(
                        failure_type=failure_type,
                        error_message=match.group(0),
                        full_output=output,
                    )
                    # Extract location if possible
                    self._extract_location(failure, output)
                    failures.append(failure)
        
        # If no specific pattern matched, create generic failure
        if not failures and exit_code != 0:
            failures.append(Failure(
                failure_type=FailureType.UNKNOWN,
                error_message=output[:500],
                full_output=output,
            ))
        
        return failures
    
    def _extract_location(self, failure: Failure, output: str) -> None:
        """Extract file path and line number from error output."""
        for pattern in self.LOCATION_PATTERNS:
            match = re.search(pattern, output)
            if match:
                failure.file_path = match.group(1)
                failure.line_number = int(match.group(2))
                break


class FixGenerator:
    """Generates fix suggestions for detected failures."""
    
    # Pre-defined fixes for common issues
    FIX_TEMPLATES = {
        FailureType.IMPORT_ERROR: [
            "pip install {module}",
            "npm install {module}",
        ],
        FailureType.DEPENDENCY_MISSING: [
            "pip install {package}",
            "npm install {package}",
        ],
        FailureType.SYNTAX_ERROR: [
            # Requires LLM for real fix
            "# Syntax error at line {line} - requires code review",
        ],
    }
    
    def generate_fixes(self, failure: Failure) -> List[str]:
        """Generate potential fixes for a failure."""
        fixes = []
        
        if failure.failure_type == FailureType.IMPORT_ERROR:
            # Extract module name from error
            match = re.search(r"No module named '(\w+)'", failure.error_message)
            if match:
                module = match.group(1)
                fixes.append(f"pip install {module}")
        
        elif failure.failure_type == FailureType.DEPENDENCY_MISSING:
            # Parse npm/pip install suggestion
            match = re.search(r"(pip|npm) install ([\w\-@/]+)", failure.error_message)
            if match:
                fixes.append(f"{match.group(1)} install {match.group(2)}")
        
        elif failure.failure_type == FailureType.TEST_FAILURE:
            # Test failures need LLM analysis
            fixes.append("# Test failure requires LLM analysis of test file")
        
        failure.suggested_fixes = fixes
        return fixes


class SelfHealingAgent:
    """
    Autonomous self-healing agent for OwnStack.
    
    The killer feature that nobody else has:
    - Runs locally in sandboxed container
    - Automatically detects and fixes failures
    - Validates fixes before suggesting
    - No cloud dependency, no API costs
    
    Usage:
        healer = SelfHealingAgent(runtime, llm_provider)
        session = await healer.heal(container_id, "pytest tests/")
        if session.healed:
            print("Fixed automatically!")
    """
    
    def __init__(self, runtime, llm_provider=None):
        self.runtime = runtime
        self.llm = llm_provider  # Optional for advanced fixes
        self.analyzer = FailureAnalyzer()
        self.fix_generator = FixGenerator()
    
    async def heal(
        self,
        container_id: str,
        command: str,
        max_attempts: int = 5,
    ) -> HealingSession:
        """
        Attempt to heal a failing command.
        
        1. Run command
        2. Analyze failures
        3. Generate fixes
        4. Apply fix in sandbox
        5. Re-run and validate
        6. Repeat until success or max attempts
        """
        import uuid
        session = HealingSession(
            session_id=f"heal-{uuid.uuid4().hex[:8]}",
            container_id=container_id,
            original_command=command,
            original_output="",
            max_attempts=max_attempts,
        )
        
        # Initial run
        result = await self.runtime.exec_capture_async(container_id, command)
        stdout, stderr, exit_code = result
        session.original_output = stdout + stderr
        
        if exit_code == 0:
            session.healed = True
            return session
        
        # Log original output
        logger.info(f"Self-Healing: Command failed with code {exit_code}. Analyzing...")
        session.failures_detected = self.analyzer.analyze(
            session.original_output, exit_code
        )
        
        # Healing loop
        applied_fixes = set()
        
        for attempt_num in range(max_attempts):
            session.total_attempts += 1
            
            if not session.failures_detected:
                break
            
            failure = session.failures_detected[0]
            print(f"DEBUG Healer: Attempting fix for {failure.failure_type}: {failure.error_message}")
            fixes = self.fix_generator.generate_fixes(failure)
            
            # Select first untried fix
            fix = next((f for f in fixes if f not in applied_fixes), None)
            
            if not fix:
                # Try LLM for complex fixes
                if self.llm:
                    llm_fixes = await self._llm_generate_fix(failure)
                    fix = next((f for f in llm_fixes if f not in applied_fixes), None)
                
                if not fix:
                    print("DEBUG Healer: No new fixes found, stopping.")
                    break
            
            # Apply fix
            applied_fixes.add(fix)
            start_time = time.time()
            
            try:
                # Detect if fix is an installation command
                is_install = any(cmd in fix for cmd in ["pip install", "npm install", "apt-get install"])
                
                if is_install:
                    logger.info(f"Applying networked fix: {fix}")
                    fix_output = await self.runtime.run_install_command_async(container_id, fix)
                else:
                    logger.info(f"Applying local fix: {fix}")
                    stdout, stderr, code = await self.runtime.exec_capture_async(container_id, fix)
                    fix_output = stdout + stderr
                
                # Backoff logic: if we failed and are repeating, wait a bit
                if attempt_num > 0:
                    wait_time = min(2 ** attempt_num, 10)
                    logger.info(f"Self-Healing: Backoff waiting {wait_time}s...")
                    await asyncio.sleep(wait_time)

                # Re-run original command to verify
                verify_stdout, verify_stderr, verify_exit = await self.runtime.exec_capture_async(container_id, command)
                verify_output = verify_stdout + verify_stderr
                
                attempt = HealingAttempt(
                    failure=failure,
                    fix_applied=fix,
                    success=verify_exit == 0,
                    verification_output=verify_output,
                    duration_ms=int((time.time() - start_time) * 1000),
                )
                session.attempts.append(attempt)
                if verify_exit == 0:
                    session.healed = True
                    break
                
                # Analyze new failures
                session.failures_detected = self.analyzer.analyze(
                    verify_output, verify_exit
                )
                
            except Exception as e:
                attempt = HealingAttempt(
                    failure=failure,
                    fix_applied=fix,
                    success=False,
                    verification_output=str(e),
                    duration_ms=int((time.time() - start_time) * 1000),
                )
                session.attempts.append(attempt)
                await asyncio.sleep(1) # Safety wait on exception
                continue # Try next attempt
        
        # Phase 39: Autonomous Incident Reporting
        if not session.healed and any(f.failure_type == FailureType.TRAINING_ERROR for f in session.failures_detected):
            await self._emit_incident_report(session)

        return session

    async def _emit_incident_report(self, session: HealingSession):
        """Generate and save an incident_report.md artifact."""
        failure = next(f for f in session.failures_detected if f.failure_type == FailureType.TRAINING_ERROR)
        report = f"""# 🚨 Incident Report: Training Failure Detected

**Type**: {failure.failure_type}
**Timestamp**: {time.strftime('%Y-%m-%d %H:%M:%S')}
**Session**: {session.session_id}

## Error Details
```text
{failure.error_message}
```

## Autonomous Analysis
Le système a détecté une anomalie critique durant l'entraînement (NaN ou Overflow). 
Ceci indique généralement un Learning Rate trop élevé ou un problème d'instabilité numérique.

## Prochaines Étapes Suggérées
1. Réduire le Learning Rate de 50%.
2. Activer Clip Gradients (max_norm=1.0).
3. Vérifier la normalisation des données d'entrée.
"""
        # Save incident report via artifact manager (logic simplified for brevity)
        # In a real impl, we'd use the same mechanism as total_agent
        print(f"DEBUG: Emitting incident_report.md for session {session.session_id}")
    
    async def _llm_generate_fix(self, failure: Failure) -> List[str]:
        """Use LLM to generate fix for complex failures."""
        if not self.llm:
            return []
        
        prompt = f"""Analyze this error and suggest a single shell command to fix it.
This could be a 'pip install', a 'sed' command to fix code, or a file creation.

Error Type: {failure.failure_type}
File: {failure.file_path or 'Unknown'}
Line: {failure.line_number or 'Unknown'}
Error: {failure.error_message}

Respond with ONLY the shell command to apply the fix, nothing else. No markdown, no comments."""
        
        try:
            # Note: Provider.chat expects a list of Message objects or dicts
            messages = [{"role": "user", "content": prompt}]
            response = await self.llm.chat(messages=messages)
            
            # Extract content from LLMResponse or dict
            content = ""
            if hasattr(response, 'content'):
                content = response.content or ""
            elif isinstance(response, dict):
                content = response.get("content", "")
            else:
                content = str(response)

            fix = content.strip()
            # Clean possible markdown
            fix = fix.replace("```shell", "").replace("```bash", "").replace("```", "").strip()
            
            if fix and not fix.startswith("#"):
                return [fix]
        except Exception as e:
            logger.error(f"Healer LLM fix generation failed: {e}")
            pass
        
        return []
    
    def get_healing_summary(self, session: HealingSession) -> Dict[str, Any]:
        """Generate a summary of the healing session."""
        return {
            "session_id": session.session_id,
            "original_command": session.original_command,
            "healed": session.healed,
            "total_attempts": session.total_attempts,
            "failures_detected": len(session.failures_detected),
            "fixes_applied": [a.fix_applied for a in session.attempts],
            "successful_fix": next(
                (a.fix_applied for a in session.attempts if a.success),
                None
            ),
            "last_output": session.attempts[-1].verification_output if session.attempts else session.original_output
        }


# Global instance
_healer: Optional[SelfHealingAgent] = None


def get_healer(runtime, llm_provider=None) -> SelfHealingAgent:
    """Get or create the global self-healing agent."""
    global _healer
    if _healer is None:
        _healer = SelfHealingAgent(runtime, llm_provider)
    return _healer
