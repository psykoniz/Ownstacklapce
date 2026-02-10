"""
Preflight checks for OwnStack startup.

Validates environment before launching agents:
- Docker availability
- API keys presence
- Auto-detection of provider type
"""
import os
import logging
import subprocess
from dataclasses import dataclass
from typing import Optional, Tuple

logger = logging.getLogger("ownstack.preflight")


class PreflightError(Exception):
    """Raised when preflight checks fail."""
    pass


@dataclass
class PreflightResult:
    """Result of preflight checks."""
    docker_ok: bool
    api_key_ok: bool
    detected_provider: Optional[str]
    detected_model: Optional[str]
    api_key: Optional[str]
    errors: list[str]
    
    @property
    def ok(self) -> bool:
        return self.docker_ok and self.api_key_ok


def detect_provider_from_key(api_key: str) -> Tuple[str, str]:
    """
    Auto-detect provider and default model from API key prefix.
    
    Returns:
        Tuple of (provider_name, default_model)
    """
    if not api_key:
        return ("unknown", "unknown")
    
    if api_key.startswith("sk-or-"):
        return ("openrouter", "openai/gpt-4o")
    elif api_key.startswith("sk-ant-"):
        return ("anthropic", "claude-3-5-sonnet-20241022")
    elif api_key.startswith("sk-"):
        return ("openai", "gpt-4o")
    elif api_key.startswith("ollama-") or "localhost" in api_key.lower():
        return ("ollama", "llama3")
    else:
        # Default to OpenRouter as it's the most flexible
        return ("openrouter", "openai/gpt-4o")


def check_docker() -> Tuple[bool, str]:
    """
    Check if Docker is available and running.
    
    Returns:
        Tuple of (is_available, error_message)
    """
    try:
        result = subprocess.run(
            ["docker", "version", "--format", "{{.Server.Version}}"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0:
            version = result.stdout.strip()
            logger.info(f"Docker detected: v{version}")
            return (True, "")
        else:
            error = result.stderr.strip() or "Docker daemon not responding"
            return (False, error)
    except FileNotFoundError:
        return (False, "Docker not installed. Please install Docker Desktop.")
    except subprocess.TimeoutExpired:
        return (False, "Docker command timed out. Is Docker Desktop running?")
    except Exception as e:
        return (False, f"Docker check failed: {str(e)}")


def check_api_keys() -> Tuple[bool, Optional[str], str, str]:
    """
    Check for API keys in environment and auto-detect provider.
    
    Returns:
        Tuple of (is_present, api_key, provider, model)
    """
    # Priority order: OpenRouter > Anthropic > OpenAI
    keys_to_check = [
        ("OPENROUTER_API_KEY", "openrouter"),
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
    ]
    
    for env_var, expected_provider in keys_to_check:
        api_key = os.getenv(env_var)
        if api_key:
            provider, model = detect_provider_from_key(api_key)
            logger.info(f"Found {env_var} -> detected provider: {provider}")
            return (True, api_key, provider, model)
    
    return (False, None, "unknown", "unknown")


def run_preflight() -> PreflightResult:
    """
    Run all preflight checks.
    
    Returns:
        PreflightResult with status of all checks
    """
    errors = []
    
    # Check Docker
    docker_ok, docker_error = check_docker()
    if not docker_ok:
        errors.append(f"Docker: {docker_error}")
    
    # Check API Keys
    api_key_ok, api_key, provider, model = check_api_keys()
    if not api_key_ok:
        errors.append(
            "No API key found. Set one of: OPENROUTER_API_KEY, ANTHROPIC_API_KEY, or OPENAI_API_KEY"
        )
    
    # SOTA Phase 75: Check Secret Key Strength
    secret_key = os.getenv("OWNSTACK_SECRET_KEY", "")
    if not secret_key or secret_key == "os-dev-secret-change-me":
        # We don't block start (dual auth strategy), but we warn
        logger.warning("SECURITY WARNING: Using default/weak OWNSTACK_SECRET_KEY. JWTs will be insecure.")
        if secret_key: # If it's the default one, we might want to flag it but not error
             pass
    elif len(secret_key) < 32:
         logger.warning("SECURITY WARNING: OWNSTACK_SECRET_KEY is too short (<32 chars).")

    result = PreflightResult(
        docker_ok=docker_ok,
        api_key_ok=api_key_ok,
        detected_provider=provider if api_key_ok else None,
        detected_model=model if api_key_ok else None,
        api_key=api_key,
        errors=errors
    )
    
    if result.ok:
        logger.info(f"Preflight OK: Docker ready, using {provider}/{model}")
    else:
        for error in errors:
            logger.error(f"Preflight FAILED: {error}")
    
    return result


def require_preflight() -> PreflightResult:
    """
    Run preflight and raise if checks fail.
    
    Raises:
        PreflightError if any check fails
    """
    result = run_preflight()
    if not result.ok:
        raise PreflightError("\n".join(result.errors))
    return result


# Convenience function for quick validation
def validate_environment() -> bool:
    """Quick check if environment is ready. Returns True/False."""
    result = run_preflight()
    return result.ok
