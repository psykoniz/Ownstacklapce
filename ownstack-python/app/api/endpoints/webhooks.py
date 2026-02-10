from __future__ import annotations

from fastapi import APIRouter, Header, HTTPException, Request

from app.core.globals import SETTINGS
from app.utils.security import RateLimiter, verify_github_signature

router = APIRouter(prefix="/webhooks", tags=["webhooks"])
limiter = RateLimiter(SETTINGS.webhook_rate_limit, SETTINGS.webhook_rate_window_s)


@router.post("/github")
async def github_webhook(
    request: Request,
    x_hub_signature_256: str | None = Header(default=None),
    x_github_event: str | None = Header(default=None),
) -> dict:
    if not limiter.allow(request.client.host if request.client else "unknown"):
        raise HTTPException(status_code=429, detail="rate limit exceeded")
    if x_github_event not in {"push", "issues"}:
        raise HTTPException(status_code=400, detail="event not allowed")
    if not SETTINGS.github_webhook_secret:
        raise HTTPException(status_code=500, detail="webhook secret not configured")
    body = await request.body()
    if not verify_github_signature(SETTINGS.github_webhook_secret, body, x_hub_signature_256 or ""):
        raise HTTPException(status_code=401, detail="invalid signature")
    return {"status": "accepted", "event": x_github_event}
