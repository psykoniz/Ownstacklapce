"""Path safety utilities for sandbox containment."""
from __future__ import annotations

import os

class PathSafetyError(Exception):
    """Raised when a path operation is unsafe."""
    pass


# Files that should never be accessed
DENIED_FILES = {".env", "id_rsa", "id_ed25519", ".ssh", "config"}
DENIED_EXTENSIONS = {".pem", ".key"}


def validate_path(root: str, path: str) -> str:
    """
    Validate that 'path' is safe and contained within 'root'.
    Returns the absolute resolved path.
    Raises PathSafetyError if unsafe.
    """
    abs_root = os.path.abspath(root)
    
    # Normalize path - handle both absolute container paths and relative paths
    if path.startswith("/workspace"):
        # Container absolute path - make it relative
        path = path[len("/workspace"):].lstrip("/")
    
    joined = os.path.join(abs_root, path)
    resolved = os.path.abspath(joined)
    
    # Containment check
    if not resolved.startswith(abs_root + os.sep) and resolved != abs_root:
        raise PathSafetyError(f"Path traversal detected: {path}")
        
    # Check for secrets
    filename = os.path.basename(resolved)
    if filename in DENIED_FILES:
        raise PathSafetyError(f"Access to protected file denied: {filename}")
    
    _, ext = os.path.splitext(filename)
    if ext.lower() in DENIED_EXTENSIONS:
        raise PathSafetyError(f"Access to protected file type denied: {ext}")
    
    # Check for .git secrets
    if ".git" in resolved and filename in {"config", "credentials"}:
        raise PathSafetyError("Access to git credentials denied")
          
    return resolved


def validate_read_path(root: str, path: str) -> str:
    """Validate a path for read operations."""
    return validate_path(root, path)


def validate_write_path(root: str, path: str) -> str:
    """Validate a path for write operations."""
    resolved = validate_path(root, path)
    
    # Additional write restrictions
    filename = os.path.basename(resolved)
    if filename.startswith(".git"):
        raise PathSafetyError("Cannot write to .git directory")
        
    return resolved

