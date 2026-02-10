from typing import AsyncGenerator, Optional, Any
from app.agent.core import BaseAgent, AgentConfig

logger = logging.getLogger(__name__)

class NativeAgentBridge:
    """Bridge to the local SOTA BaseAgent for mission execution."""
    
    def __init__(self, session_id: str, runtime: Any, container_id: str, rules_loader: Any = None):
        self.session_id = session_id
        self.runtime = runtime
        self.container_id = container_id
        self.rules_loader = rules_loader

    async def run_task(self, task: str, role: str = "engineer") -> AsyncGenerator[str, None]:
        """
        Execute a coding task using the internal BaseAgent.
        Yields status updates and final response.
        """
        config = AgentConfig(
            role=role,
            max_steps=15, # Missions usually need more depth
            model_tiering=True
        )
        
        agent = BaseAgent(
            session_id=self.session_id,
            runtime=self.runtime,
            container_id=self.container_id,
            config=config,
            rules_loader=self.rules_loader
        )
        
        logger.info(f"Launching Native Agent for mission task in container {self.container_id}")
        
        async for event in agent.run_stream(task):
            if event.event == "step":
                yield f"\n[AGENT STEP {event.data['step']}]\n"
            elif event.event == "tool_start":
                yield f"-> Executing tool: {event.data['name']}...\n"
            elif event.event == "tool_end":
                if event.data.get("error"):
                    yield f" [ERROR] {event.data['error']}\n"
                else:
                    yield " [SUCCESS]\n"
            elif event.event == "complete":
                yield f"\n\n[MISSION COMPLETED]\n{event.data['response']}\n"
            elif event.event == "error":
                yield f"\n[FATAL ERROR] {event.data['message']}\n"

class ClaudeCodeBridge:
    """Bridge to the Anthropic 'claude' CLI tool."""
    
    def __init__(self, executable: str = "claude"):
        self.executable = executable

    async def run_task(self, task: str, cwd: str) -> AsyncGenerator[str, None]:
        """
        Execute a coding task using Claude Code CLI.
        Yields stdout/stderr chunks.
        """
        # Detection: use global 'claude' if exists, otherwise fallback to npx
        # Detection: use where.exe on Windows, which on Linux
        cmd_args = []
        if self.executable == "claude":
            checker = "where.exe" if os.name == "nt" else "which"
            try:
                # Check if 'claude' exists in path
                subprocess.run([checker, "claude"], capture_output=True, check=True, shell=(os.name == "nt"))
                cmd_args = ["claude"]
            except subprocess.CalledProcessError:
                npx = "npx.cmd" if os.name == "nt" else "npx"
                cmd_args = [npx, "-y", "@anthropic-ai/claude-code"]
        else:
            cmd_args = [self.executable]

        # Add generic flags
        cmd_args.append("-p")
        
        logger.info(f"Launching Claude Code (via STDIN): {' '.join(cmd_args)} in {cwd}")
        
        process = await asyncio.create_subprocess_exec(
            cmd_args[0],
            *cmd_args[1:],
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=cwd,
            env={**os.environ, "CLAUDE_CODE_NON_INTERACTIVE": "1"}
        )

        # Send the task via STDIN and close it
        if process.stdin:
            process.stdin.write(task.encode())
            await process.stdin.drain()
            process.stdin.close()
            await process.stdin.wait_closed()

        async def _read_stream(stream, prefix=""):
            while True:
                line = await stream.read(1024)
                if not line:
                    break
                yield line.decode(errors="replace")

        # Merge stdout and stderr for the feedback loop
        async for chunk in self._merge_streams(process.stdout, process.stderr):
            yield chunk

        return_code = await process.wait()
        if return_code != 0:
            yield f"\n[ERROR] Claude Code exited with code {return_code}\n"

    async def _merge_streams(self, stdout, stderr):
        """Simple merge of two async streams."""
        async def _read(s):
            while True:
                chunk = await s.read(1024)
                if not chunk: break
                yield chunk

        # Parallel read using tasks
        # (For simplicity in this bridge, we'll just alternate or use a queue if needed)
        # Here we just read stdout then stderr for now, 
        # but in a real mission we want them mixed.
        
        while True:
            done = True
            # Try stdout
            try:
                chunk = await asyncio.wait_for(stdout.read(1024), timeout=0.1)
                if chunk:
                    yield chunk.decode(errors="replace")
                    done = False
            except asyncio.TimeoutError:
                pass
            
            # Try stderr
            try:
                chunk = await asyncio.wait_for(stderr.read(1024), timeout=0.1)
                if chunk:
                    yield f"[STDERR] {chunk.decode(errors='replace')}"
                    done = False
            except asyncio.TimeoutError:
                pass
            
            if done and stdout.at_eof() and stderr.at_eof():
                break
