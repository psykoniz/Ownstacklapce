"""Structured error responses for consistent API error handling."""
from __future__ import annotations

from typing import Any, Dict, Optional

from fastapi import HTTPException, Request
from fastapi.responses import JSONResponse
from pydantic import BaseModel


class ErrorDetail(BaseModel):
    """Structured error response model."""
    code: str
    message: str
    details: Optional[Dict[str, Any]] = None
    path: Optional[str] = None
    session_id: Optional[str] = None


class APIError(HTTPException):
    """Custom API exception with structured error details."""
    
    def __init__(
        self,
        status_code: int = 500, # Default to 500 if not specified
        code: str = "INTERNAL_ERROR",
        message: str = "An error occurred",
        details: Optional[Dict[str, Any]] = None,
        session_id: Optional[str] = None,
    ):
        self.code = code
        self.error_message = message
        self.details = details
        self.session_id = session_id
        super().__init__(status_code=status_code, detail=message)
    
    def to_response(self, path: str = "") -> ErrorDetail:
        return ErrorDetail(
            code=self.code,
            message=self.error_message,
            details=self.details,
            path=path,
            session_id=self.session_id,
        )

# Alias for internal use
AppError = APIError


# Common error codes
class ErrorCodes:
    # Auth errors (401)
    AUTH_MISSING_KEY = "AUTH_MISSING_KEY"
    AUTH_INVALID_KEY = "AUTH_INVALID_KEY"
    
    # Session errors (404)
    SESSION_NOT_FOUND = "SESSION_NOT_FOUND"
    RUN_NOT_FOUND = "RUN_NOT_FOUND"
    
    # Policy errors (400)
    POLICY_DENIED = "POLICY_DENIED"
    POLICY_REQUIRES_APPROVAL = "POLICY_REQUIRES_APPROVAL"
    
    # Path errors (400)
    PATH_TRAVERSAL = "PATH_TRAVERSAL"
    PATH_DENIED = "PATH_DENIED"
    
    # Resource errors (404)
    RESOURCE_NOT_FOUND = "RESOURCE_NOT_FOUND"
    
    # Tool errors (500)
    TOOL_EXECUTION_ERROR = "TOOL_EXECUTION_ERROR"
    LSP_ERROR = "LSP_ERROR"
    RUNTIME_ERROR = "RUNTIME_ERROR"
    INTERNAL_ERROR = "INTERNAL_ERROR"
    
    # SOTA Phase 74: Rate limiting errors (429)
    RATE_LIMITED = "RATE_LIMITED"
    SESSION_LIMIT_EXCEEDED = "SESSION_LIMIT_EXCEEDED"


async def api_error_handler(request: Request, exc: APIError) -> JSONResponse:
    """Global exception handler for APIError."""
    return JSONResponse(
        status_code=exc.status_code,
        content=exc.to_response(path=str(request.url.path)).model_dump(),
    )
