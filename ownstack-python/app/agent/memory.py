"""OwnStack Project Memory - Rules Engine.

Reads `.ownstack/rules.md` and injects project-specific rules into agent prompts.
This is OwnStack's "Mémoire de Projet" - the agent learns your preferences.
"""
from __future__ import annotations

import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional
import hashlib


@dataclass
class ProjectRules:
    """Parsed project rules from .ownstack/rules.md"""
    
    # Raw content
    raw_content: str = ""
    content_hash: str = ""
    
    # Parsed sections
    coding_style: List[str] = field(default_factory=list)
    forbidden: List[str] = field(default_factory=list)
    preferences: List[str] = field(default_factory=list)
    libraries: List[str] = field(default_factory=list)
    testing: List[str] = field(default_factory=list)
    knowledge: List[str] = field(default_factory=list)
    custom_sections: Dict[str, List[str]] = field(default_factory=dict)
    
    # SOTA Phase 73: Priority & Relevance
    priority_weights: Dict[str, float] = field(default_factory=lambda: {
        "forbidden": 1.0,  # Always inject
        "coding_style": 0.8,
        "testing": 0.7,
        "preferences": 0.6,
        "libraries": 0.5,
        "knowledge": 0.4,
    })
    
    def get_prioritized_rules(self, task_context: str = "") -> List[tuple]:
        """
        SOTA: Return rules sorted by priority, optionally filtered by relevance to task.
        Returns list of (rule_text, priority_score) tuples.
        """
        all_rules = []
        
        # Add forbidden rules (always highest priority)
        for rule in self.forbidden:
            all_rules.append((f"❌ FORBIDDEN: {rule}", 1.0))
        
        # Add other rules with priority weighting
        for rule in self.coding_style:
            all_rules.append((f"Style: {rule}", self.priority_weights["coding_style"]))
        for rule in self.testing:
            all_rules.append((f"Test: {rule}", self.priority_weights["testing"]))
        for rule in self.preferences:
            all_rules.append((f"Prefer: {rule}", self.priority_weights["preferences"]))
        for rule in self.libraries:
            all_rules.append((f"Use: {rule}", self.priority_weights["libraries"]))
        for rule in self.knowledge:
            all_rules.append((f"💡 {rule}", self.priority_weights["knowledge"]))
        
        # Simple keyword relevance boost (foundation for RAG)
        if task_context:
            task_lower = task_context.lower()
            boosted = []
            for rule, priority in all_rules:
                # Boost rules that contain keywords from the task
                keywords = task_lower.split()
                relevance_boost = sum(0.1 for kw in keywords if kw in rule.lower() and len(kw) > 3)
                boosted.append((rule, min(1.0, priority + relevance_boost)))
            all_rules = boosted
        
        # Sort by priority (highest first)
        return sorted(all_rules, key=lambda x: x[1], reverse=True)
    
    def to_system_prompt(self, task_context: str = "", max_rules: int = 20) -> str:
        """
        SOTA: Convert rules to optimized system prompt with priority filtering.
        """
        prioritized = self.get_prioritized_rules(task_context)
        
        # Take top N rules to avoid context bloat
        top_rules = prioritized[:max_rules]
        
        lines = ["## Project Rules (from .ownstack/rules.md)"]
        
        for rule, priority in top_rules:
            # Add priority indicator for high-priority rules
            if priority >= 0.9:
                lines.append(f"- 🚨 {rule}")
            else:
                lines.append(f"- {rule}")
        
        if len(prioritized) > max_rules:
            lines.append(f"\n_({len(prioritized) - max_rules} lower-priority rules omitted)_")
        
        return "\n".join(lines)
    
    def is_empty(self) -> bool:
        return not any([
            self.coding_style,
            self.forbidden,
            self.preferences,
            self.libraries,
            self.testing,
            self.custom_sections,
        ])


def parse_rules_md(content: str) -> ProjectRules:
    """Parse .ownstack/rules.md content into structured rules."""
    rules = ProjectRules(
        raw_content=content,
        content_hash=hashlib.md5(content.encode()).hexdigest()[:8],  # nosec: content hash only
    )
    
    # Section mapping (case-insensitive)
    section_map = {
        "coding style": "coding_style",
        "style": "coding_style",
        "forbidden": "forbidden",
        "never": "forbidden",
        "don't": "forbidden",
        "preferences": "preferences",
        "prefer": "preferences",
        "libraries": "libraries",
        "deps": "libraries",
        "dependencies": "libraries",
        "testing": "testing",
        "tests": "testing",
        "knowledge": "knowledge",
        "memory": "knowledge",
        "discoveries": "knowledge",
    }
    
    current_section = None
    current_items: List[str] = []
    
    for line in content.split("\n"):
        line = line.strip()
        
        # Skip empty lines
        if not line:
            continue
        
        # Detect section headers (# or ##)
        header_match = re.match(r"^#{1,2}\s+(.+)$", line)
        if header_match:
            # Save previous section
            if current_section and current_items:
                section_key = section_map.get(current_section.lower())
                if section_key:
                    getattr(rules, section_key).extend(current_items)
                else:
                    rules.custom_sections[current_section] = current_items
            
            current_section = header_match.group(1).strip()
            current_items = []
            continue
        
        # Detect list items (- or *)
        item_match = re.match(r"^[-*]\s+(.+)$", line)
        if item_match and current_section:
            item_text = item_match.group(1).strip()
            # Handle [P0] Priority
            if "[P0]" in item_text.upper():
                item_text = "🚨 " + item_text.replace("[P0]", "").replace("[p0]", "").strip()
            
            current_items.append(item_text)
    
    # Save last section
    if current_section and current_items:
        section_key = section_map.get(current_section.lower())
        if section_key:
            getattr(rules, section_key).extend(current_items)
        else:
            rules.custom_sections[current_section] = current_items
    
    return rules


class RulesLoader:
    """Loads and caches project rules with hot-reload support."""
    
    
    def __init__(self, workspace_root: str):
        self.workspace_root = Path(workspace_root)
        self.agents_md_path = self.workspace_root / "AGENTS.md"
        self.legacy_rules_path = self.workspace_root / ".ownstack" / "rules.md"
        self._cached_rules: Optional[ProjectRules] = None
        self._cached_mtime: float = 0
        self._active_path: Optional[Path] = None
    
    def _resolve_path(self) -> Optional[Path]:
        """Resolve the active rules file."""
        if self.agents_md_path.exists():
            return self.agents_md_path
        if self.legacy_rules_path.exists():
            return self.legacy_rules_path
        return None

    def get_rules(self) -> ProjectRules:
        """Get rules, reloading if file changed."""
        active_path = self._resolve_path()
        if not active_path:
            return ProjectRules()
        
        current_mtime = active_path.stat().st_mtime
        
        # Check if we switched files or file changed
        path_changed = active_path != self._active_path
        
        if self._cached_rules and not path_changed and current_mtime == self._cached_mtime:
            return self._cached_rules
        
        # Reload
        content = active_path.read_text(encoding="utf-8")
        self._cached_rules = parse_rules_md(content)
        
        # If loading AGENTS.md, we might want to adapt parsing logic if format differs significantly
        # But ProjectRules parser is generic enough for Markdown headers
        
        self._cached_mtime = current_mtime
        self._active_path = active_path
        return self._cached_rules
    
    def has_rules(self) -> bool:
        return self._resolve_path() is not None


# Default AGENTS.md template
DEFAULT_AGENTS_MD_TEMPLATE = """# AGENTS.md

This file provides context and instructions for AI coding agents (OwnStack).

## Coding Guidelines
- Use descriptive variable names.
- Add docstrings to all public functions.
- Keep functions under 50 lines.
- Prefer async/await over callbacks.

## Testing Instructions
- Run `pytest` for unit tests.
- Run `python scripts/verify/verify_full_stack.py` for full verification.
- Add tests for new features.

## Security Policy
- No hardcoded secrets or API keys.
- Do not modify `.github/workflows` without explicit approval.
- Audit dependencies using safety check.

## Architecture
- Backend: FastAPI (app/)
- Agent: Orchestrator + Specialists (app/agent/)
- Sandbox: Docker (app/runtime/)

"""


def create_default_rules(workspace_root: str) -> Path:
    """Create default AGENTS.md if no rules exist."""
    root = Path(workspace_root)
    agents_md = root / "AGENTS.md"
    legacy_rules = root / ".ownstack" / "rules.md"
    
    if agents_md.exists():
        return agents_md
    
    if legacy_rules.exists():
        return legacy_rules
        
    # Create new standard AGENTS.md
    agents_md.write_text(DEFAULT_AGENTS_MD_TEMPLATE, encoding="utf-8")
    return agents_md
