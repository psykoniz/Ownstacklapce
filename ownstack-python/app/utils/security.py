"""Security helpers for webhooks, rate limiting, and log sanitization.

SOTA Phase 79: Log sanitization for sensitive data protection.
"""
from __future__ import annotations

import hashlib
import hmac
import re
import time
from collections import defaultdict, deque
from typing import Any, Deque, Dict


# SOTA Phase 79: Sensitive field patterns
SENSITIVE_PATTERNS = [
    re.compile(r"(api[_-]?key|apikey)", re.IGNORECASE),
    re.compile(r"(secret|password|token|credential)", re.IGNORECASE),
    re.compile(r"(authorization|auth)", re.IGNORECASE),
    re.compile(r"(bearer\s+\S+)", re.IGNORECASE),
    re.compile(r"(sk-[a-zA-Z0-9]+)", re.IGNORECASE),  # OpenAI keys
    re.compile(r"(ghp_[a-zA-Z0-9]+)", re.IGNORECASE),  # GitHub PATs
]


class RateLimiter:
    def __init__(self, limit: int, window_s: int) -> None:
        self.limit = limit
        self.window_s = window_s
        self.requests: Dict[str, Deque[float]] = defaultdict(deque)

    def allow(self, key: str) -> bool:
        now = time.time()
        queue = self.requests[key]
        while queue and now - queue[0] > self.window_s:
            queue.popleft()
        if len(queue) >= self.limit:
            return False
        queue.append(now)
        return True


def verify_github_signature(secret: str, body: bytes, signature: str) -> bool:
    if not signature:
        return False
    expected = "sha256=" + hmac.new(secret.encode("utf-8"), body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, signature)


def validate_command(command: str) -> bool:
    lowered = command.strip().lower()
    banned = ("rm ", "sudo", "curl", "wget", "ssh", "scp", "ftp")
    if any(token in lowered for token in banned):
        return False
    return True


def sanitize_log_data(data: Dict[str, Any], redact_value: str = "[REDACTED]") -> Dict[str, Any]:
    """
    SOTA Phase 79: Recursively sanitize sensitive fields in log data.
    
    Redacts values for keys matching sensitive patterns (api_key, token, secret, etc.)
    and also scrubs known secret formats from string values.
    """
    if not isinstance(data, dict):
        return data
    
    sanitized = {}
    for key, value in data.items():
        # Check if key matches sensitive pattern
        is_sensitive = any(pattern.search(key) for pattern in SENSITIVE_PATTERNS)
        
        if is_sensitive:
            sanitized[key] = redact_value
        elif isinstance(value, dict):
            sanitized[key] = sanitize_log_data(value, redact_value)
        elif isinstance(value, str):
            # Scrub known secret formats from string values
            scrubbed = value
            for pattern in SENSITIVE_PATTERNS:
                scrubbed = pattern.sub(redact_value, scrubbed)
            sanitized[key] = scrubbed
        elif isinstance(value, list):
            sanitized[key] = [
                sanitize_log_data(item, redact_value) if isinstance(item, dict) else item
                for item in value
            ]
        else:
            sanitized[key] = value
    
    return sanitized

