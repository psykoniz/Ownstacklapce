"""Context7 Integration - Real Documentation Fetching via Secure Browser.

Replaces fictional API calls with real browser automation to fetch
documentation from official sources.
"""
from __future__ import annotations

import asyncio
import hashlib
import json
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional
import re

# Import Secure Browser
from app.tools.browser import get_secure_browser, SecureBrowser

# Cache settings
import os
try:
    # Try to find project root
    PROJECT_ROOT = Path(__file__).parent.parent.parent.parent.parent
except:
    PROJECT_ROOT = Path(".")

CACHE_DIR = PROJECT_ROOT / ".ownstack" / "docs-cache"
CACHE_TTL = 86400  # 24 hours


@dataclass
class LibraryDoc:
    """Documentation snippet."""
    library_id: str
    title: str
    content: str
    url: str
    fetched_at: float = 0


class Context7Client:
    """
    Documentation Fetcher powered by Secure Browser.
    
    Uses Playwright to visit official documentation sites and
    extract readable content for the agent.
    """
    
    def __init__(self, cache_dir: Path = CACHE_DIR):
        self.cache_dir = cache_dir
        self.browser: Optional[SecureBrowser] = None
        self._cache: Dict[str, LibraryDoc] = {}
        
        # Mapping of common libs to their doc types
        self.doc_registry = {
            "fastapi": "https://fastapi.tiangolo.com/",
            "pydantic": "https://docs.pydantic.dev/latest/",
            "react": "https://react.dev/reference/react",
            "nextjs": "https://nextjs.org/docs",
            "tailwindcss": "https://tailwindcss.com/docs",
            "docker": "https://docs.docker.com/",
            "playwright": "https://playwright.dev/python/docs/intro",
            "pytest": "https://docs.pytest.org/en/stable/",
            "sqlalchemy": "https://docs.sqlalchemy.org/en/20/",
            "pandas": "https://pandas.pydata.org/docs/",
            "numpy": "https://numpy.org/doc/stable/",
            "python": "https://docs.python.org/3/",
        }

    async def _get_browser(self) -> SecureBrowser:
        """Lazy load browser."""
        if not self.browser:
            self.browser = get_secure_browser()
            await self.browser.start()
        return self.browser

    async def get_library_docs(
        self,
        library: str,
        topic: Optional[str] = None,
        max_tokens: int = 5000,
    ) -> List[LibraryDoc]:
        """
        Fetch documentation for a library using Secure Browser.
        """
        library = library.lower().strip()
        topic_key = topic.lower().strip() if topic else "main"
        cache_key = f"{library}:{topic_key}"
        
        # 1. Check Memory Cache
        if cache_key in self._cache:
            if time.time() - self._cache[cache_key].fetched_at < CACHE_TTL:
                return [self._cache[cache_key]]
        
        # 2. Check Disk Cache
        disk_doc = self._load_from_disk(cache_key)
        if disk_doc:
            self._cache[cache_key] = disk_doc
            return [disk_doc]
            
        # 3. Fetch Real Docs
        try:
            url = self._resolve_url(library, topic)
            if not url:
                return [LibraryDoc(
                    library_id=library,
                    title=f"Unknown library: {library}",
                    content=f"No documentation URL found for {library}. Please check the name.",
                    url="",
                    fetched_at=time.time()
                )]
            
            browser = await self._get_browser()
            print(f"Context7: Browsing {url}...")
            
            result = await browser.browse(url)
            
            if "error" in result:
                raise Exception(result["error"])
                
                
            doc = LibraryDoc(
                library_id=library,
                title=result.get("title", library),
                content=result.get("text_content", "")[:max_tokens], # Truncate for now
                url=url,
                fetched_at=time.time()
            )
            
            # Save to caches
            self._cache[cache_key] = doc
            self._save_to_disk(cache_key, doc)
            
            return [doc]
            
        except Exception as e:
            return [LibraryDoc(
                library_id=library,
                title=f"Error fetching {library}",
                content=f"Failed to fetch docs: {str(e)}",
                url="",
                fetched_at=time.time()
            )]

    def _resolve_url(self, library: str, topic: Optional[str]) -> Optional[str]:
        """Resolve library+topic to a URL."""
        base_url = self.doc_registry.get(library)
        if not base_url:
            return None
            
        if not topic:
            return base_url
            
        # Simple heuristic for topics (can be improved)
        # e.g., fastapi + "security" -> https://fastapi.tiangolo.com/tutorial/security/
        if library == "fastapi":
            return f"{base_url}tutorial/{topic}/"
        elif library == "react":
            return f"{base_url}/{topic}"
            
        return base_url

    def _save_to_disk(self, key: str, doc: LibraryDoc) -> None:
        """Save doc to disk."""
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        safe_key = hashlib.md5(key.encode()).hexdigest()
        file_path = self.cache_dir / f"{safe_key}.json"
        
        data = {
            "library_id": doc.library_id,
            "title": doc.title,
            "content": doc.content,
            "url": doc.url,
            "fetched_at": doc.fetched_at
        }
        file_path.write_text(json.dumps(data, indent=2), encoding="utf-8")

    def _load_from_disk(self, key: str) -> Optional[LibraryDoc]:
        """Load doc from disk."""
        safe_key = hashlib.md5(key.encode()).hexdigest()
        file_path = self.cache_dir / f"{safe_key}.json"
        
        if not file_path.exists():
            return None
            
        try:
            data = json.loads(file_path.read_text(encoding="utf-8"))
            if time.time() - data["fetched_at"] > CACHE_TTL:
                return None
                
            return LibraryDoc(
                library_id=data["library_id"],
                title=data["title"],
                content=data["content"],
                url=data["url"],
                fetched_at=data["fetched_at"]
            )
        except:
            return None

    async def close(self):
        """Cleanup browser resources."""
        if self.browser:
            await self.browser.stop()


# Global client
_client: Optional[Context7Client] = None

def get_context7_client() -> Context7Client:
    """Get or create the global Context7 client."""
    global _client
    if _client is None:
        _client = Context7Client()
    return _client
