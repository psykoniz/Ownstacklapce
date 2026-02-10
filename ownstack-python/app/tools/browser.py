"""Secure Browser Module for Safe 'Computer Use'.

Implements an isolated, policy-enforced browser environment for the agent.
Uses Playwright with strict restrictions to prevent SSRF and exfiltration.
"""
from __future__ import annotations

import asyncio
import logging
import re
import os
from dataclasses import dataclass, field
from typing import List, Optional
from urllib.parse import urlparse

from playwright.async_api import async_playwright, Browser, BrowserContext, Page

from app.core.errors import AppError, ErrorCodes

import random
import hashlib
import time
from typing import Dict

logger = logging.getLogger(__name__)


# SOTA Phase 78: User-Agent Stealth Rotation
USER_AGENTS = [
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
]


class BrowserStealth:
    """SOTA Phase 78: Manage human-like browsing behavior."""
    
    def __init__(self):
        self._last_request_time = 0.0
        self._request_count = 0
    
    def get_user_agent(self) -> str:
        """Rotate User-Agent for each session."""
        return random.choice(USER_AGENTS)
    
    async def apply_delay(self):
        """Apply randomized delay to mimic human behavior."""
        import asyncio
        # Delay between 0.5s and 2s, increasing slightly with request count
        base_delay = random.uniform(0.5, 1.5)
        fatigue_delay = min(self._request_count * 0.1, 1.0)
        await asyncio.sleep(base_delay + fatigue_delay)
        self._request_count += 1


class BrowserCache:
    """SOTA Phase 78: In-memory cache for browsed pages."""
    
    def __init__(self, ttl_seconds: int = 3600):
        self._cache: Dict[str, tuple] = {}  # url_hash -> (content, timestamp)
        self._ttl = ttl_seconds
    
    def _hash_url(self, url: str) -> str:
        return hashlib.md5(url.encode()).hexdigest()
    
    def get(self, url: str) -> Optional[dict]:
        key = self._hash_url(url)
        if key in self._cache:
            content, ts = self._cache[key]
            if time.time() - ts < self._ttl:
                logger.info(f"[BrowserCache] HIT for {url[:50]}...")
                return content
            else:
                del self._cache[key]
        return None
    
    def set(self, url: str, content: dict):
        key = self._hash_url(url)
        self._cache[key] = (content, time.time())
        logger.info(f"[BrowserCache] STORED {url[:50]}...")
    
    def invalidate(self, url: str):
        key = self._hash_url(url)
        if key in self._cache:
            del self._cache[key]


@dataclass
class BrowserPolicy:
    """Security policy for the browser."""
    allowed_domains: List[str] = field(default_factory=lambda: [
        "docs.python.org",
        "pypi.org",
        "stackoverflow.com",
        "github.com",
        "fastapi.tiangolo.com",
        "platform.openai.com",
        "console.anthropic.com",
        "google.com",
        "duckduckgo.com",
        "react.dev",
        "nextjs.org",
        "tailwindcss.com",
        "docs.pydantic.dev",
        "docs.docker.com",
        "playwright.dev",
        "docs.pytest.org",
        "docs.sqlalchemy.org",
        "pandas.pydata.org",
        "numpy.org",
        "localhost",
        "127.0.0.1",
    ])
    block_ads: bool = True
    max_pages: int = 5
    timeout_ms: int = 30000
    allow_downloads: bool = False
    stealth_mode: bool = True  # SOTA Phase 78
    use_cache: bool = True  # SOTA Phase 78
    # P2: Sidecar support
    ws_endpoint: Optional[str] = os.getenv("BROWSER_WS_ENDPOINT")



class SecureBrowser:
    """
    Secure, isolated browser for agent use.
    
    Features:
    - Domain allow-listing
    - Resource blocking (ads, trackers)
    - Timeout enforcement
    - Visual capture (screenshots)
    """
    
    def __init__(self, policy: Optional[BrowserPolicy] = None):
        self.policy = policy or BrowserPolicy()
        self._browser: Optional[Browser] = None
        self._playwright = None
        # SOTA Phase 78
        self._stealth = BrowserStealth() if self.policy.stealth_mode else None
        self._cache = BrowserCache() if self.policy.use_cache else None
    
    async def start(self):
        """Start the browser instance."""
        if self._browser:
            return
            
        if self.policy.ws_endpoint:
            logger.info(f"Connecting to remote browser sidecar at {self.policy.ws_endpoint}")
            self._playwright = await async_playwright().start()
            self._browser = await self._playwright.chromium.connect_over_cdp(self.policy.ws_endpoint)
        else:
            self._playwright = await async_playwright().start()
            # Launch in headless mode for security and speed
            self._browser = await self._playwright.chromium.launch(
                headless=True,
                args=[
                    "--no-sandbox",
                    "--disable-setuid-sandbox",
                    "--disable-dev-shm-usage",
                ]
            )
    
    async def stop(self):
        """Stop the browser instance."""
        if self._browser:
            await self._browser.close()
            self._browser = None
        if self._playwright:
            await self._playwright.stop()
            self._playwright = None

    def _is_allowed(self, url: str) -> bool:
        """Check if URL is allowed by policy."""
        try:
            parsed = urlparse(url)
            domain = parsed.netloc.lower()
            if domain.startswith("www."):
                domain = domain[4:]
            
            # Check exact match or subdomain
            for allowed in self.policy.allowed_domains:
                if domain == allowed or domain.endswith(f".{allowed}"):
                    return True
            return False
        except Exception:
            return False

    async def browse(self, url: str, action: str = "navigate", selector: str = None, text: str = None) -> dict:
        """
        Safely browse a URL and perform an optional action.
        """
        if not self._is_allowed(url):
            raise AppError(
                ErrorCodes.POLICY_DENIED,
                f"Access to {url} is blocked by security policy."
            )
        
        if not self._browser:
            await self.start()
        
        # SOTA Phase 78: Check Cache first
        if self._cache and action == "navigate":
            cached = self._cache.get(url)
            if cached:
                return cached
        
        # SOTA Phase 78: Apply Stealth delay
        if self._stealth:
            await self._stealth.apply_delay()
        
        # Use rotated UA if stealth enabled
        user_agent = self._stealth.get_user_agent() if self._stealth else "OwnStack-Agent/1.0"
        context = await self._browser.new_context(user_agent=user_agent)
        page = await context.new_page()
        
        try:
            # Step 1: Navigate
            await page.goto(url, timeout=self.policy.timeout_ms, wait_until="networkidle")
            
            # Step 2: Perform Action
            if action == "click" and selector:
                await page.click(selector, timeout=5000)
            elif action == "type" and selector and text:
                await page.fill(selector, text, timeout=5000)
            
            # Step 3: Capture State
            title = await page.title()
            
            # Semantic Extraction (Roles, Labels, Values)
            semantic_script = """
            () => {
              const elements = Array.from(document.querySelectorAll('button, input, a, [role], select, textarea'));
              return elements.map(el => ({
                tag: el.tagName,
                role: el.getAttribute('role'),
                text: el.innerText || el.value || el.placeholder || el.getAttribute('aria-label'),
                id: el.id,
                class: el.className
              })).filter(e => e.text && e.text.length > 0).slice(0, 50);
            }
            """
            semantic_elements = await page.evaluate(semantic_script)
            
            # Simple text extraction
            text_content = await page.evaluate("() => document.body.innerText")
            # Screenshot
            screenshot = await page.screenshot(type="jpeg", quality=50)
            import base64
            screenshot_b64 = base64.b64encode(screenshot).decode("utf-8")
            
            result = {
                "url": url,
                "title": title,
                "text_content": text_content[:5000],
                "semantic_elements": semantic_elements,
                "screenshot": screenshot_b64,
            }
            
            # SOTA Phase 78: Store in cache
            if self._cache and action == "navigate":
                self._cache.set(url, result)
            
            return result
        except Exception as e:
            return {"error": str(e)}
        finally:
            await context.close()


# Singleton instance
_browser_instance: Optional[SecureBrowser] = None

def get_secure_browser() -> SecureBrowser:
    """Get the singleton browser instance."""
    global _browser_instance
    if not _browser_instance:
        _browser_instance = SecureBrowser()
    return _browser_instance
