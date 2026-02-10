from fastapi import APIRouter
from app.core.globals import STATE
import time
import os

try:
    import psutil
except ImportError:
    psutil = None

router = APIRouter(prefix="/monitoring", tags=["monitoring"])

@router.get("/health")
async def health_check():
    """Basic health check and system status."""
    process_memory = 0
    if psutil:
        try:
            process = psutil.Process(os.getpid())
            process_memory = process.memory_info().rss / 1024 / 1024
        except:
            pass
    
    # Check Docker connectivity
    docker_ok = False
    try:
        await STATE.runtime.client.ping()
        docker_ok = True
    except:
        pass
        
    return {
        "status": "ok" if docker_ok else "degraded",
        "timestamp": time.time(),
        "backend": {
            "memory_usage_mb": round(process_memory, 2),
            "uptime_seconds": round(time.time() - STATE.runtime.start_time, 1) if hasattr(STATE.runtime, "start_time") else 0
        },
        "docker": {
            "connected": docker_ok,
            "active_containers": len(await STATE.runtime.client.containers.list(filters={"label": "ownstack-agent"})) if docker_ok else 0
        },
        "sessions": {
            "active_count": await STATE.session_manager.get_active_count()
        }
    }
