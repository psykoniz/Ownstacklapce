"""API Key & JWT authentication middleware for OwnStack IDE."""
from __future__ import annotations

import os
from typing import Optional, Union, Dict, Any
from datetime import datetime, timedelta

from fastapi import Request, HTTPException, Depends, status
from fastapi.security import APIKeyHeader, OAuth2PasswordBearer
from pydantic import BaseModel

# SOTA Phase 75: Import Security Config
from app.core.globals import SECURITY_CONFIG

# Graceful degradation for JWT dependencies
try:
    from jose import JWTError, jwt
    from passlib.context import CryptContext
    JWT_AVAILABLE = True
    pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")
except ImportError:
    JWT_AVAILABLE = False
    jwt = None
    pwd_context = None
    print("WARNING: python-jose or passlib not installed. JWT auth disabled.")

# Define schemes
API_KEY_HEADER = APIKeyHeader(name="X-API-Key", auto_error=False)
OAUTH2_SCHEME = OAuth2PasswordBearer(tokenUrl="token", auto_error=False)

# Phase 75: Auth Constants
_API_KEY = os.getenv("OWNSTACK_API_KEY")


# Models
class Token(BaseModel):
    access_token: str
    token_type: str
    expires_at: Optional[datetime] = None

class TokenData(BaseModel):
    username: Optional[str] = None
    role: str = "user"


def is_auth_enabled() -> bool:
    """Check if authentication is enabled."""
    return bool(_API_KEY) or bool(SECURITY_CONFIG.SECRET_KEY)


# ========== Helpers ==========

def verify_password(plain_password, hashed_password):
    if not JWT_AVAILABLE: return False
    return pwd_context.verify(plain_password, hashed_password)

def get_password_hash(password):
    if not JWT_AVAILABLE: return ""
    return pwd_context.hash(password)

def create_access_token(data: dict, expires_delta: Optional[timedelta] = None):
    if not JWT_AVAILABLE:
        raise HTTPException(500, "JWT libraries not installed")
    
    to_encode = data.copy()
    if expires_delta:
        expire = datetime.utcnow() + expires_delta
    else:
        expire = datetime.utcnow() + timedelta(minutes=15)
    
    to_encode.update({"exp": expire})
    encoded_jwt = jwt.encode(to_encode, SECURITY_CONFIG.SECRET_KEY, algorithm=SECURITY_CONFIG.ALGORITHM)
    return encoded_jwt


# ========== Main Auth Dependency ==========

async def get_current_user(
    request: Request,
    api_key: Optional[str] = Depends(API_KEY_HEADER),
    token: Optional[str] = Depends(OAUTH2_SCHEME)
) -> Dict[str, Any]:
    """
    SOTA Dual Auth Strategy:
    1. Check API Key (Dev Mode, high priority for scripts)
    2. Check JWT (User Mode, for frontend)
    """
    
    # 1. API Key Check
    # We check header manually too because Depends(API_KEY_HEADER) might return None if not present
    header_key = request.headers.get("X-API-Key")
    if header_key == _API_KEY:
        return {"username": "dev-user", "role": "admin", "auth_method": "api_key"}

    # 2. JWT Check
    if token and JWT_AVAILABLE:
        try:
            payload = jwt.decode(token, SECURITY_CONFIG.SECRET_KEY, algorithms=[SECURITY_CONFIG.ALGORITHM])
            username: str = payload.get("sub")
            role: str = payload.get("role", "user")
            if username is None:
                raise HTTPException(status_code=401, detail="Invalid token payload")
            return {"username": username, "role": role, "auth_method": "jwt"}
        except JWTError:
            # If token provided but invalid, we might want to fail
            # But let's check if API key was attempted and failed
            pass
            
    # 3. Fallback / Failure
    # If auth is disabled (no API key set), allowing access is dangerous in SOTA context
    # But sticking to legacy behavior: if _API_KEY is unset, we are in insecure dev mode
    if not _API_KEY:
        return {"username": "anon", "role": "admin", "auth_method": "none"}

    raise HTTPException(
        status_code=status.HTTP_401_UNAUTHORIZED,
        detail="Not authenticated. Provide X-API-Key or Bearer Token.",
        headers={"WWW-Authenticate": "Bearer"},
    )


# Legacy alias for backward compatibility
async def verify_api_key(request: Request) -> None:
    """
    Legacy dependency. Wraps get_current_user but ignores return.
    """
    try:
        await get_current_user(request)
    except HTTPException:
        raise
