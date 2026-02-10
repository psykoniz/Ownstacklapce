import re
import os
from typing import List, Dict, Any

class ArtifactManager:
    """Manages the extraction and persistence of agent artifacts (plans, todos, etc.)."""
    
    def __init__(self, workspace_root: str):
        self.workspace_root = workspace_root
        self.artifacts_rel_dir = os.path.join(".ownstack", "artifacts")

    def extract_artifacts(self, text: str) -> List[Dict[str, str]]:
        """Extract artifacts from XML-like tags in the text."""
        if not text:
            return []
            
        # Phase 66: Robust regex allowing extra attributes (e.g. for security/XSS safety)
        # Matches <artifact type="name" [name="subname"] [other="..."]>content</artifact>
        pattern = re.compile(
            r'<artifact\s+[^>]*type="([^"]+)"(?:[^>]*name="([^"]+)")?[^>]*>(.*?)</artifact>', 
            re.DOTALL | re.IGNORECASE
        )
        matches = pattern.findall(text)
        
        artifacts = []
        for type_id, name, content in matches:
            artifacts.append({
                "type": type_id,
                "name": name or type_id,
                "content": content.strip()
            })
        return artifacts

    async def save_artifacts(self, artifacts: List[Dict[str, str]], runtime: Any, container_id: str):
        """Save extracted artifacts to the workspace via runtime."""
        if not artifacts:
            return
            
        # Ensure dir exists in container
        await runtime.exec_capture_async(container_id, f"mkdir -p {self.artifacts_rel_dir}")
        
        for artifact in artifacts:
            # Phase 40 & 66: Standardized naming and strict sanitization
            type_id = artifact['type'].upper()
            
            # Anti-Naïvety: Strict filename cleaning (no path traversal)
            raw_name = artifact['name'].lower().replace(" ", "_")
            name = "".join(c for c in raw_name if c.isalnum() or c == "_")
            if not name:
                name = "unnamed"
            
            # Map specific types to standardized filenames
            if type_id == "PLAN":
                filename = "plan.md"
            elif type_id == "TODO":
                filename = "todo.md"
            elif type_id == "PROOF":
                filename = "proof.md"
            elif type_id == "SCRATCHPAD":
                filename = "scratchpad.md"
            elif type_id == name.upper():
                filename = f"{type_id.lower()}.md"
            else:
                filename = f"{type_id.lower()}_{name}.md"
            
            path = os.path.join(self.artifacts_rel_dir, filename).replace("\\", "/") 
            
            try:
                import anyio
                await anyio.to_thread.run_sync(runtime.write_file, container_id, path, artifact['content'])
            except Exception as e:
                print(f"ERROR: Failed to save artifact {filename}: {e}")
