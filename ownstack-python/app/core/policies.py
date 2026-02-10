"""Command policy engine with regex-based pattern matching."""
from __future__ import annotations

import re
import os
import json
from pathlib import Path
from datetime import datetime
from typing import Literal, List, Pattern, Optional, Dict
from enum import Enum

PolicyDecision = Literal["ALLOW", "DENY", "ASK"]


class SecurityLevel(str, Enum):
    """Security level for the session."""
    STRICT = "strict"      # Block network/install unless whitelisted
    STANDARD = "standard"  # Ask for network/install


# SOTA Phase 73: Policy Audit Logger
class PolicyAuditLogger:
    """Logs all policy decisions for security auditing."""
    
    _instance: Optional['PolicyAuditLogger'] = None
    
    def __init__(self, log_dir: str = ".ownstack/policy_logs"):
        self.log_dir = Path(log_dir)
        self.log_dir.mkdir(parents=True, exist_ok=True)
        self.log_file = self.log_dir / f"policy_{datetime.now().strftime('%Y%m%d')}.jsonl"
    
    @classmethod
    def get_instance(cls) -> 'PolicyAuditLogger':
        if cls._instance is None:
            workspace = os.getenv("WORKSPACE_ROOT", ".")
            cls._instance = cls(f"{workspace}/.ownstack/policy_logs")
        return cls._instance
    
    def log(self, command: str, decision: PolicyDecision, reason: str = ""):
        """Log a policy decision."""
        entry = {
            "timestamp": datetime.now().isoformat(),
            "command": command[:200],  # Truncate for safety
            "decision": decision,
            "reason": reason,
        }
        try:
            with open(self.log_file, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry) + "\n")
        except Exception:
            pass  # Fail silently for logging


# SOTA Phase 73: Dynamic Policy Loader
class DynamicPolicyConfig:
    """Load policies from YAML/JSON config file."""
    
    _cached_config: Optional[Dict] = None
    _cached_mtime: float = 0
    
    @classmethod
    def load(cls, config_path: str = ".ownstack/policies.yaml") -> Dict:
        """Load policy config with caching."""
        path = Path(config_path)
        
        # Check if we have a valid cache
        if path.exists():
            mtime = path.stat().st_mtime
            if cls._cached_config and mtime == cls._cached_mtime:
                return cls._cached_config
            
            # Load config
            try:
                import yaml
                with open(path, "r", encoding="utf-8") as f:
                    cls._cached_config = yaml.safe_load(f)
                    cls._cached_mtime = mtime
                    return cls._cached_config
            except ImportError:
                # Fallback to JSON
                json_path = Path(config_path.replace(".yaml", ".json"))
                if json_path.exists():
                    with open(json_path, "r", encoding="utf-8") as f:
                        cls._cached_config = json.load(f)
                        return cls._cached_config
            except Exception:
                pass
        
        # Return default config
        return {"deny_patterns": [], "ask_patterns": [], "allowed_domains": []}


# Whitelisted domains for STRICT mode
ALLOWED_DOMAINS = [
    "pypi.org", "files.pythonhosted.org",  # pip
    "registry.npmjs.org",                  # npm
    "github.com",                          # git
    "vscode-update.azurewebsites.net",     # vscode
]


def _compile_patterns(patterns: List[str]) -> List[Pattern]:
    """Compile patterns for robust matching."""
    compiled = []
    for p in patterns:
        escaped = re.escape(p)
        if any(c in p for c in "/-*>|"):
            compiled.append(re.compile(escaped, re.IGNORECASE))
        else:
            try:
                compiled.append(re.compile(rf"\b{escaped}\b", re.IGNORECASE))
            except re.error:
                compiled.append(re.compile(escaped, re.IGNORECASE))
    return compiled


# High-risk commands that should never be allowed
DENY_PATTERNS_RAW = [
    r"rm\s+-(?:r|f|rf|fr)\s+/",   # rm -rf /
    r"mkfs",                      # formatting
    r"dd\s+if=",                  # direct disk access
    r":\(\)\s*\{\s*:\|:&",        # fork bomb
    r"sudo\s+",
    r"su\s+",
    r"chown\s+root",
    r"chmod\s+(?:777|000)",       # Dangerous permissions
    r">+\s*/etc/",                # Overwriting system config
    r">+\s*/boot/",
    r">+\s*/proc/",
    r">+\s*/sys/",
    r"/dev/sd[a-z]",              # Raw device access
    r"nc\s+.*-e",                 # Netcat reverse shell
    r"bash\s+-i",                 # Interactive shell
    r"python\s+-c\s+.*pty",       # PTY spawn
    r"ssh\s+",                    # No outbound SSH
    r"shutdown",                  # No system shutdown
    r"reboot",                    # No system reboot
]

# Commands requiring user approval (network/install)
ASK_PATTERNS_RAW = [
    r"pip\s+install",
    r"npm\s+install",
    r"yarn\s+add",
    r"curl\s+",
    r"wget\s+",
    r"git\s+push",
    r"git\s+clone",
    r"docker\s+",
]

DENY_PATTERNS = [re.compile(p, re.IGNORECASE) for p in DENY_PATTERNS_RAW]
ASK_PATTERNS = [re.compile(p, re.IGNORECASE) for p in ASK_PATTERNS_RAW]


def check_command(command: str, level: SecurityLevel = SecurityLevel.STANDARD) -> PolicyDecision:
    """
    Check if a command is allowed, denied, or requires approval.
    SOTA Phase 73: Integrated audit logging.
    """
    # Normalize whitespace
    cmd_normalized = " ".join(command.split())
    
    # Get audit logger
    audit = PolicyAuditLogger.get_instance()
    
    # 1. Check DENY patterns (Always blocked)
    for pattern in DENY_PATTERNS:
        if pattern.search(cmd_normalized):
            audit.log(cmd_normalized, "DENY", f"Matched deny pattern: {pattern.pattern}")
            return "DENY"
            
    # 2. Check ASK patterns (Risky commands)
    requires_approval = False
    matched_pattern = None
    for pattern in ASK_PATTERNS:
        if pattern.search(cmd_normalized):
            requires_approval = True
            matched_pattern = pattern.pattern
            break
    
    if requires_approval:
        if level == SecurityLevel.STRICT:
            # In STRICT mode, we might allow if domain is whitelisted
            # This is a simple heuristic check
            if _is_whitelisted(cmd_normalized):
                audit.log(cmd_normalized, "ALLOW", "Whitelisted domain in STRICT mode")
                return "ALLOW"
            audit.log(cmd_normalized, "DENY", f"STRICT mode, non-whitelisted: {matched_pattern}")
            return "DENY" # Block non-whitelisted risky commands in strict mode
        else:
            audit.log(cmd_normalized, "ASK", f"Matched ask pattern: {matched_pattern}")
            return "ASK" # In STANDARD mode, just ask user
    
    # ALLOW without logging (too noisy for normal commands)
    return "ALLOW"



def _is_whitelisted(command: str) -> bool:
    """Check if command uses only whitelisted domains (STRICT PARSING)."""
    import shlex
    from urllib.parse import urlparse

    try:
        # Split command respecting quotes
        parts = shlex.split(command)
    except ValueError:
        return False  # Malformed command -> Block

    # Find potential URLs or Hostnames in arguments
    found_domains = []
    
    for arg in parts:
        # Check standard URLs
        if arg.startswith(("http://", "https://")):
            try:
                parsed = urlparse(arg)
                if parsed.netloc:
                    found_domains.append(parsed.netloc)
            except Exception:
                pass
        
        # Check direct domain usage (e.g. ping github.com)
        elif "." in arg and not arg.startswith(("/", "-", ".")):
             found_domains.append(arg)

    if not found_domains:
        return False # No domain found context but mandatory for whitelist -> Block

    # Verify ALL found domains are in whitelist
    for found in found_domains:
        # Remove port if present
        clean_found = found.split(":")[0].lower()
        
        is_allowed = False
        for allowed in ALLOWED_DOMAINS:
            # Exact match or subdomain
            if clean_found == allowed or clean_found.endswith("." + allowed):
                is_allowed = True
                break
        
        if not is_allowed:
            return False

    return True


def check_command_batch(commands: List[str], level: SecurityLevel = SecurityLevel.STANDARD) -> List[PolicyDecision]:
    """Check multiple commands at once."""
    return [check_command(cmd, level) for cmd in commands]
