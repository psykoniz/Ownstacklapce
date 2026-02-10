"""RepoMap v2 Runner with intelligent caching.

Improvements over v1:
- In-memory LRU cache for hot files
- Persistent file-based cache with mtime invalidation
- Batch parsing for multiple files
- Incremental updates (only parse changed files)
"""
from __future__ import annotations

import hashlib
import json
import textwrap
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

from app.runtime.manager import RuntimeManager
# We keep this import for type checking or host-side utility, 
# even if strict dependency is removed for the container script.
from app.tools.repomap_v2 import (
    query_for_language_v2,
    symbols_to_dict,
    build_call_graph,
)


@dataclass
class CacheEntry:
    """Cached parsing result for a single file."""
    mtime: float
    content_hash: str
    symbols: List[Dict[str, Any]]


class RepoMapCache:
    """Intelligent multi-level cache for RepoMap results."""
    
    def __init__(self, cache_path: str = "/cache/repomap_v2.json"):
        self.cache_path = Path(cache_path)
        self._memory_cache: Dict[str, CacheEntry] = {}
        self._disk_cache: Optional[Dict[str, Any]] = None
    
    def load_disk_cache(self) -> Dict[str, Any]:
        """Load persistent cache from disk."""
        if self._disk_cache is not None:
            return self._disk_cache
        
        if not self.cache_path.exists():
            self._disk_cache = {"version": 2, "files": {}}
            return self._disk_cache
        
        try:
            self._disk_cache = json.loads(self.cache_path.read_text(encoding="utf-8"))
            # Check version compatibility
            if self._disk_cache.get("version") != 2:
                self._disk_cache = {"version": 2, "files": {}}
        except (json.JSONDecodeError, IOError):
            self._disk_cache = {"version": 2, "files": {}}
        
        return self._disk_cache
    
    def save_disk_cache(self) -> None:
        """Persist cache to disk."""
        if self._disk_cache is None:
            return
        
        try:
            self.cache_path.parent.mkdir(parents=True, exist_ok=True)
            self.cache_path.write_text(
                json.dumps(self._disk_cache, ensure_ascii=False),
                encoding="utf-8"
            )
        except IOError:
            pass  # Cache write failure is non-fatal
    
    def get(self, file_path: str, mtime: float, content_hash: str) -> Optional[List[Dict[str, Any]]]:
        """Get cached symbols if still valid."""
        # Check memory cache first (fastest)
        if file_path in self._memory_cache:
            entry = self._memory_cache[file_path]
            if entry.mtime == mtime and entry.content_hash == content_hash:
                return entry.symbols
        
        # Check disk cache
        disk_cache = self.load_disk_cache()
        if file_path in disk_cache["files"]:
            entry = disk_cache["files"][file_path]
            if entry.get("mtime") == mtime and entry.get("hash") == content_hash:
                symbols = entry.get("symbols", [])
                # Promote to memory cache
                self._memory_cache[file_path] = CacheEntry(mtime, content_hash, symbols)
                return symbols
        
        return None
    
    def put(self, file_path: str, mtime: float, content_hash: str, symbols: List[Dict[str, Any]]) -> None:
        """Store symbols in both caches."""
        # Memory cache
        self._memory_cache[file_path] = CacheEntry(mtime, content_hash, symbols)
        
        # Disk cache
        disk_cache = self.load_disk_cache()
        disk_cache["files"][file_path] = {
            "mtime": mtime,
            "hash": content_hash,
            "symbols": symbols
        }
    
    def get_stats(self) -> Dict[str, int]:
        """Get cache statistics."""
        disk_cache = self.load_disk_cache()
        return {
            "memory_entries": len(self._memory_cache),
            "disk_entries": len(disk_cache.get("files", {}))
        }


def content_hash(data: str) -> str:
    """Fast content hash for cache invalidation."""
    return hashlib.md5(data.encode("utf-8")).hexdigest()[:16]


async def generate_repomap_v2(runtime: RuntimeManager, container_id: str) -> Dict[str, Any]:
    """
    Generate RepoMap v2 with enhanced symbol extraction and caching.
    
    Returns:
        Dict with:
        - files: list of file entries with symbols
        - call_graph: mapping of functions to their callees
        - cache_stats: cache hit/miss statistics
    """
    # 1. Read the repomap_v2 library code from the backend source
    try:
        repomap_lib_path = Path(__file__).parent / "repomap_v2.py"
        repomap_lib_code = repomap_lib_path.read_text(encoding="utf-8")
    except Exception as e:
        raise RuntimeError(f"Failed to read repomap_v2.py library: {e}")

    # 2. Define the runner script that uses the library
    runner_logic = textwrap.dedent(
        """
        import hashlib
        import json
        import os
        import subprocess
        import sys
        from pathlib import Path
        
        # NOTE: 'repomap_v2' classes and functions are already defined above 
        # because we concatenated the library code.

        CACHE_PATH = Path("/cache/repomap_v2.json")
        ROOT = Path("/workspace")

        EXTENSION_LANG = {
            ".py": "python",
            ".ts": "typescript",
            ".tsx": "typescript",
            ".js": "javascript",
            ".jsx": "javascript",
            ".c": "cpp",
            ".cc": "cpp",
            ".cpp": "cpp",
            ".h": "cpp",
            ".hpp": "cpp",
        }

        def load_cache():
            if not CACHE_PATH.exists():
                return {"version": 2, "files": {}}
            try:
                cache = json.loads(CACHE_PATH.read_text(encoding="utf-8"))
                if cache.get("version") != 2:
                    return {"version": 2, "files": {}}
                return cache
            except Exception:
                return {"version": 2, "files": {}}

        def save_cache(cache):
            CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
            CACHE_PATH.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")

        def content_hash(data):
            return hashlib.md5(data.encode("utf-8")).hexdigest()[:16]

        def parse_file(path, language, source):
            query = query_for_language_v2(language)
            if not query:
                return []
            
            # Write query to temp file for tree-sitter
            query_file = Path(f"/tmp/query_{language}.scm")
            query_file.write_text(query, encoding="utf-8")
            
            result = subprocess.run(
                ["tree-sitter", "query", "--captures", str(query_file), str(path)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            if result.returncode != 0:
                return []
            
            captures = list(parse_captures(result.stdout))
            symbols = extract_symbols_v2(source, captures)
            return symbols_to_dict(symbols)

        cache = load_cache()
        files_cache = cache.get("files", {})
        updated = {}
        all_symbols = []
        stats = {"hits": 0, "misses": 0}

        for root, dirs, files in os.walk(ROOT):
            # Skip hidden directories and blacklisted folders
            dirs[:] = [d for d in dirs if not d.startswith(".") and d not in (
                "node_modules", "__pycache__", "venv", ".venv", "env", ".env", "dist", "build", "coverage", ".git"
            )]
            
            for name in files:
                ext = Path(name).suffix
                language = EXTENSION_LANG.get(ext)
                if not language:
                    continue
                    
                path = Path(root) / name
                
                try:
                    rel_path = str(path.relative_to(ROOT))
                except ValueError:
                    continue

                try:
                    stat = path.stat()
                    mtime = stat.st_mtime
                    if stat.st_size > 1024 * 1024: continue # Skip large files

                    source = path.read_text(encoding="utf-8", errors="replace")
                    file_hash = content_hash(source)
                except (FileNotFoundError, PermissionError, OSError):
                    continue
                
                # Check cache
                cached = files_cache.get(rel_path)
                if cached and cached.get("mtime") == mtime and cached.get("hash") == file_hash:
                    updated[rel_path] = cached
                    all_symbols.extend([{"file": rel_path, **s} for s in cached.get("symbols", [])])
                    stats["hits"] += 1
                    continue
                
                # Parse file
                symbols = parse_file(path, language, source)
                updated[rel_path] = {"mtime": mtime, "hash": file_hash, "symbols": symbols}
                all_symbols.extend([{"file": rel_path, **s} for s in symbols])
                stats["misses"] += 1

        cache["files"] = updated
        save_cache(cache)

        # Build file list with symbols
        files_output = []
        for rel_path, entry in updated.items():
            files_output.append({
                "path": rel_path,
                "language": EXTENSION_LANG.get(Path(rel_path).suffix, "unknown"),
                "symbols": entry.get("symbols", [])
            })

        payload = {
            "files": files_output,
            "total_symbols": len(all_symbols),
            "cache_stats": stats,
        }
        print(json.dumps(payload))
        """
    )
    
    # 3. Concatenate library code + runner logic
    # We strip lines from repomap_lib_code that might be problematic if any (e.g., imports of backend)
    # But since repomap_v2.py is a pure library with stdlib imports, it should be fine.
    full_script = f"{repomap_lib_code}\n\n{runner_logic}"

    command = f"python3 - <<'PY'\n{full_script}\nPY"
    stdout, stderr, code = await runtime.exec_capture_async(container_id, command)
    if code != 0:
        raise RuntimeError(f"RepoMap v2 failed: {stderr[-500:]}")
    
    try:
        return json.loads(stdout)
    except json.JSONDecodeError:
        raise RuntimeError(f"RepoMap v2 returned invalid JSON: {stdout[:200]}...")


# Convenience function for quick symbol lookup
async def get_file_symbols(runtime: RuntimeManager, container_id: str, file_path: str) -> List[Dict[str, Any]]:
    """Get symbols for a single file (uses generate_repomap_v2 caching)."""
    result = await generate_repomap_v2(runtime, container_id)
    for f in result.get("files", []):
        if f.get("path") == file_path:
            return f.get("symbols", [])
    return []
