#!/usr/bin/env python3
"""
OwnStack Doctor — AI-powered self-healing IDE loop.

Launches the IDE in E2E mode, takes screenshots, sends them to a vision-capable
LLM for analysis, receives fix instructions, applies patches, rebuilds, and
retests — fully automated.

Usage:
    # With Claude API (recommended):
    ANTHROPIC_API_KEY=sk-... python scripts/doctor.py

    # With OpenAI-compatible provider:
    DOCTOR_PROVIDER=openai OPENAI_API_KEY=sk-... python scripts/doctor.py

    # With Claude Code (interactive — Claude Code controls the loop):
    claude "Run /home/user/Ownstacklapce/scripts/doctor.py and fix everything"

Prerequisites:
    - cargo (Rust toolchain)
    - A display server (X11/Wayland/Windows)
    - ImageMagick (xwd + convert) on Linux, or Windows with native capture
    - An API key for a vision-capable LLM (Claude 3+, GPT-4V+)
"""

import base64
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# ─── Configuration ───────────────────────────────────────────────────────────

ROOT = Path(__file__).resolve().parent.parent
_ext = ".exe" if sys.platform == "win32" else ""
_name = "ownstack-ide" if (ROOT / "target" / "debug" / f"ownstack-ide{_ext}").exists() or \
    (ROOT / "target" / "release" / f"ownstack-ide{_ext}").exists() else "lapce"
BINARY = ROOT / "target" / "release" / f"{_name}{_ext}"
BINARY_DEBUG = ROOT / "target" / "debug" / f"{_name}{_ext}"
SCREENSHOT_DIR = ROOT / ".ownstack" / "doctor"
MAX_ITERATIONS = 10
E2E_STARTUP_TIMEOUT = 60  # seconds
IDLE_TIMEOUT_MS = 5000

# Provider configuration
PROVIDER = os.environ.get("DOCTOR_PROVIDER", "anthropic")
ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
OPENAI_API_KEY = os.environ.get("OPENAI_API_KEY", "")
ANTHROPIC_MODEL = os.environ.get("DOCTOR_MODEL", "claude-sonnet-4-6")
OPENAI_MODEL = os.environ.get("DOCTOR_MODEL", "gpt-4o")


# ─── Color output ────────────────────────────────────────────────────────────

def _safe_print(msg, file=None):
    """Print with fallback for encoding errors on Windows."""
    try:
        print(msg, file=file)
    except UnicodeEncodeError:
        print(msg.encode("utf-8", errors="replace").decode("ascii", errors="replace"), file=file)

def info(msg):
    _safe_print(f"\033[1;34m[doctor]\033[0m {msg}")

def success(msg):
    _safe_print(f"\033[1;32m[doctor]\033[0m {msg}")

def warn(msg):
    _safe_print(f"\033[1;33m[doctor]\033[0m {msg}")

def error(msg):
    _safe_print(f"\033[1;31m[doctor]\033[0m {msg}", file=sys.stderr)


# ─── Build ───────────────────────────────────────────────────────────────────

def build_ide(release=False):
    """Build the IDE. Returns True on success."""
    mode = "--release" if release else ""
    info(f"Building IDE ({'release' if release else 'debug'})...")

    result = subprocess.run(
        f"cargo build {mode}".split(),
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        timeout=1800,
    )

    stderr = result.stderr or ""
    if result.returncode != 0:
        error("Build failed!")
        # Extract just the error lines
        err_lines = [l for l in stderr.splitlines() if "error" in l.lower()]
        for e in err_lines[:20]:
            error(f"  {e}")
        return False, stderr

    success("Build succeeded.")
    return True, ""


def run_tests():
    """Run the test suite. Returns (success, output)."""
    info("Running tests...")
    result = subprocess.run(
        ["cargo", "test", "-p", "ownstack-agent", "--lib"],
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        timeout=1800,
    )

    # Also run app tests
    result_app = subprocess.run(
        ["cargo", "test", "-p", "lapce-app", "--lib", "--", "ownstack"],
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        timeout=1800,
    )

    agent_ok = result.returncode == 0
    app_ok = result_app.returncode == 0

    summary = []
    for line in result.stdout.splitlines() + result_app.stdout.splitlines():
        if "test result:" in line:
            summary.append(line.strip())

    if agent_ok and app_ok:
        success(f"All tests passed: {'; '.join(summary)}")
    else:
        error("Some tests failed.")

    combined = result.stdout + result.stderr + result_app.stdout + result_app.stderr
    return agent_ok and app_ok, combined


# ─── E2E IDE control ────────────────────────────────────────────────────────

class IdeController:
    """Controls the IDE via its E2E JSON-RPC server."""

    def __init__(self, port):
        self.port = port
        self.base_url = f"http://127.0.0.1:{port}"
        self._id = 0

    def call(self, method, params=None):
        """Send a JSON-RPC call to the IDE."""
        import urllib.request
        self._id += 1
        body = json.dumps({
            "jsonrpc": "2.0",
            "id": self._id,
            "method": method,
            "params": params or {},
        }).encode()

        req = urllib.request.Request(
            self.base_url,
            data=body,
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = json.loads(resp.read())
                return data.get("result")
        except Exception as e:
            error(f"RPC call {method} failed: {e}")
            return None

    def ping(self):
        return self.call("ping")

    def get_state(self):
        return self.call("get_state")

    def get_diagnostics(self):
        return self.call("get_diagnostics")

    def screenshot(self, path):
        return self.call("screenshot", {"path": str(path)})

    def wait_idle(self, timeout_ms=IDLE_TIMEOUT_MS):
        return self.call("wait_idle", {"timeout_ms": timeout_ms})

    def run_command(self, name):
        return self.call("run_command", {"name": name})

    def open_file(self, path):
        return self.call("open_file", {"path": str(path)})


def launch_ide(workspace=None):
    """Launch the IDE in E2E mode, return (process, IdeController)."""
    binary = BINARY if BINARY.exists() else BINARY_DEBUG
    if not binary.exists():
        error(f"IDE binary not found at {binary}")
        return None, None

    env = os.environ.copy()
    env["OWNSTACK_E2E"] = "1"
    env["OWNSTACK_E2E_PORT"] = "0"
    env["OWNSTACK_WINDOW_SIZE"] = "1280x800"

    cmd = [str(binary), "--wait"]
    if workspace:
        cmd.append(str(workspace))

    info(f"Launching IDE: {' '.join(cmd)}")
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        text=True,
    )

    # Wait for E2E_READY:<port>
    deadline = time.time() + E2E_STARTUP_TIMEOUT
    port = None

    while time.time() < deadline:
        line = proc.stdout.readline()
        if not line:
            if proc.poll() is not None:
                error("IDE process exited prematurely.")
                return None, None
            time.sleep(0.1)
            continue

        line = line.strip()
        info(f"  IDE: {line}")

        if line.startswith("E2E_READY:"):
            port = int(line.split(":")[1])
            break

    if port is None:
        error(f"IDE did not report E2E_READY within {E2E_STARTUP_TIMEOUT}s")
        proc.kill()
        return None, None

    success(f"IDE ready on port {port}")
    controller = IdeController(port)
    return proc, controller


# ─── Screenshot & Vision Analysis ────────────────────────────────────────────

def _windows_screenshot(path):
    """Capture a screenshot on Windows using PowerShell and .NET."""
    ps_script = f"""
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$bmp.Save('{str(path).replace(chr(92), chr(92)+chr(92))}', [System.Drawing.Imaging.ImageFormat]::Png)
$gfx.Dispose()
$bmp.Dispose()
"""
    subprocess.run(
        ["powershell", "-NoProfile", "-Command", ps_script],
        timeout=10,
        capture_output=True,
    )
    return path.exists()


def take_screenshot(controller, iteration):
    """Take a screenshot via E2E driver. Returns path or None."""
    SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)
    path = SCREENSHOT_DIR / f"iteration_{iteration}.png"
    result = controller.screenshot(str(path))
    if result and result.get("status") == "ok":
        success(f"Screenshot saved: {path}")
        return path
    else:
        warn(f"E2E screenshot failed: {result}")
        # Fallback 1: Windows native screenshot
        if sys.platform == "win32":
            try:
                if _windows_screenshot(path):
                    success(f"Screenshot saved (Windows fallback): {path}")
                    return path
            except Exception as e:
                warn(f"Windows screenshot failed: {e}")
        # Fallback 2: scrot (Linux)
        try:
            subprocess.run(
                ["scrot", str(path)],
                timeout=5,
                capture_output=True,
            )
            if path.exists():
                success(f"Screenshot saved (scrot fallback): {path}")
                return path
        except FileNotFoundError:
            pass
        return None


def encode_image(path):
    """Read and base64-encode an image file."""
    with open(path, "rb") as f:
        return base64.standard_b64encode(f.read()).decode("utf-8")


def analyze_with_anthropic(screenshot_path, state, diagnostics, context):
    """Send screenshot + context to Claude for analysis."""
    import urllib.request

    image_data = encode_image(screenshot_path)

    messages = [{
        "role": "user",
        "content": [
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": image_data,
                },
            },
            {
                "type": "text",
                "text": f"""You are OwnStack Doctor — an AI that analyzes the OwnStack IDE
screenshot and diagnoses visual/functional issues.

## IDE State
```json
{json.dumps(state, indent=2) if state else "unavailable"}
```

## LSP Diagnostics
```json
{json.dumps(diagnostics, indent=2) if diagnostics else "none"}
```

## Previous Context
{context}

## Instructions

Analyze this screenshot of the OwnStack IDE. Look for:
1. **Visual bugs** — misaligned elements, missing text, broken layouts, wrong colors
2. **UX issues** — confusing labels, missing affordances, unclear state indicators
3. **Functional problems** — error messages visible, panels not rendering, broken features

For EACH issue found, provide:
- **issue**: Short description
- **severity**: critical / major / minor / cosmetic
- **file**: The source file to fix (e.g., lapce-app/src/ownstack_chat.rs)
- **fix**: Exact code change as an `old_text` → `new_text` replacement

If the IDE looks correct and functional, respond with: {{"status": "healthy", "issues": []}}

Respond ONLY with valid JSON:
{{
  "status": "healthy" | "issues_found",
  "summary": "one-line summary",
  "issues": [
    {{
      "issue": "description",
      "severity": "critical|major|minor|cosmetic",
      "file": "relative/path.rs",
      "fix": {{
        "old_text": "exact text to find",
        "new_text": "replacement text"
      }}
    }}
  ]
}}""",
            },
        ],
    }]

    body = json.dumps({
        "model": ANTHROPIC_MODEL,
        "max_tokens": 4096,
        "messages": messages,
    }).encode()

    api_base = os.environ.get("ANTHROPIC_BASE_URL", "https://api.anthropic.com")
    api_url = f"{api_base.rstrip('/')}/v1/messages"

    req = urllib.request.Request(
        api_url,
        data=body,
        headers={
            "x-api-key": ANTHROPIC_API_KEY,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
        },
    )

    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            data = json.loads(resp.read())
            text = data["content"][0]["text"]
            # Extract JSON from the response
            json_match = re.search(r'\{[\s\S]*\}', text)
            if json_match:
                return json.loads(json_match.group())
            return {"status": "error", "summary": "Could not parse response", "issues": []}
    except Exception as e:
        error(f"Vision analysis failed: {e}")
        return {"status": "error", "summary": str(e), "issues": []}


def analyze_with_openai(screenshot_path, state, diagnostics, context):
    """Send screenshot + context to OpenAI-compatible provider for analysis.
    Supports both Responses API (/v1/responses) and Chat Completions (/v1/chat/completions).
    """
    import urllib.request

    image_data = encode_image(screenshot_path)
    api_base = os.environ.get("OPENAI_BASE_URL", "https://api.openai.com/v1")
    wire_api = os.environ.get("DOCTOR_WIRE_API", "responses")

    prompt_text = f"""Analyze this OwnStack IDE screenshot for visual/UX/functional issues.
State: {json.dumps(state) if state else 'N/A'}
Diagnostics: {json.dumps(diagnostics) if diagnostics else 'none'}
Context: {context}
Respond with JSON: {{"status":"healthy"|"issues_found","summary":"...","issues":[{{"issue":"...","severity":"...","file":"...","fix":{{"old_text":"...","new_text":"..."}}}}]}}"""

    if wire_api == "responses":
        # OpenAI Responses API format
        body = json.dumps({
            "model": OPENAI_MODEL,
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_image",
                            "image_url": f"data:image/png;base64,{image_data}",
                        },
                        {
                            "type": "input_text",
                            "text": prompt_text,
                        },
                    ],
                },
            ],
        }).encode()
        endpoint = f"{api_base.rstrip('/')}/responses"
    else:
        # Chat Completions API format
        body = json.dumps({
            "model": OPENAI_MODEL,
            "max_tokens": 4096,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": f"data:image/png;base64,{image_data}",
                        },
                    },
                    {
                        "type": "text",
                        "text": prompt_text,
                    },
                ],
            }],
        }).encode()
        endpoint = f"{api_base.rstrip('/')}/chat/completions"

    req = urllib.request.Request(
        endpoint,
        data=body,
        headers={
            "Authorization": f"Bearer {OPENAI_API_KEY}",
            "Content-Type": "application/json",
        },
    )

    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            data = json.loads(resp.read())
            # Extract text from response based on wire format
            if wire_api == "responses":
                text = ""
                for item in data.get("output", []):
                    if item.get("type") == "message":
                        for part in item.get("content", []):
                            if part.get("type") == "output_text":
                                text += part.get("text", "")
            else:
                text = data["choices"][0]["message"]["content"]
            json_match = re.search(r'\{[\s\S]*\}', text)
            if json_match:
                return json.loads(json_match.group())
            return {"status": "error", "summary": "Could not parse response", "issues": []}
    except Exception as e:
        error(f"Vision analysis failed: {e}")
        return {"status": "error", "summary": str(e), "issues": []}


def analyze_screenshot(screenshot_path, state, diagnostics, context):
    """Route to the configured provider."""
    if PROVIDER == "anthropic":
        return analyze_with_anthropic(screenshot_path, state, diagnostics, context)
    else:
        return analyze_with_openai(screenshot_path, state, diagnostics, context)


# ─── Fix Application ────────────────────────────────────────────────────────

def apply_fixes(issues):
    """Apply code fixes from the analysis. Returns count of applied fixes."""
    applied = 0

    for issue in issues:
        fix = issue.get("fix")
        if not fix:
            warn(f"  Skipping '{issue['issue']}' — no fix provided")
            continue

        issue_file = issue.get("file")
        if not issue_file:
            warn(f"  Skipping '{issue['issue']}' — no file specified")
            continue

        file_path = ROOT / issue_file
        if not file_path.exists():
            warn(f"  File not found: {issue_file}")
            continue

        old_text = fix["old_text"]
        new_text = fix["new_text"]

        content = file_path.read_text(encoding="utf-8")
        if old_text not in content:
            warn(f"  old_text not found in {issue['file']} — skipping")
            continue

        count = content.count(old_text)
        if count > 1:
            warn(f"  old_text found {count} times in {issue['file']} — ambiguous, skipping")
            continue

        content = content.replace(old_text, new_text, 1)
        file_path.write_text(content, encoding="utf-8")
        success(f"  Fixed: {issue['issue']} [{issue['severity']}] in {issue['file']}")
        applied += 1

    return applied


# ─── Report Generation ───────────────────────────────────────────────────────

def generate_report(iterations_log):
    """Generate a markdown report of the doctor session."""
    SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)
    report_path = SCREENSHOT_DIR / "report.md"
    lines = [
        "# OwnStack Doctor Report",
        f"**Date:** {time.strftime('%Y-%m-%d %H:%M:%S')}",
        f"**Iterations:** {len(iterations_log)}",
        "",
    ]

    for i, log in enumerate(iterations_log):
        lines.append(f"## Iteration {i + 1}")
        lines.append(f"**Status:** {log['status']}")
        lines.append(f"**Summary:** {log.get('summary', 'N/A')}")
        if log.get("issues"):
            lines.append(f"**Issues found:** {len(log['issues'])}")
            for issue in log["issues"]:
                sev = issue.get("severity", "?")
                lines.append(f"- [{sev}] {issue['issue']} (`{issue.get('file', '?')}`)")
        if log.get("fixes_applied"):
            lines.append(f"**Fixes applied:** {log['fixes_applied']}")
        if log.get("build_ok") is not None:
            lines.append(f"**Build:** {'OK' if log['build_ok'] else 'FAILED'}")
        if log.get("tests_ok") is not None:
            lines.append(f"**Tests:** {'OK' if log['tests_ok'] else 'FAILED'}")
        if log.get("screenshot"):
            lines.append(f"**Screenshot:** `{log['screenshot']}`")
        lines.append("")

    final = iterations_log[-1] if iterations_log else {}
    if final.get("status") == "healthy":
        lines.append("## Result: HEALTHY")
    else:
        lines.append("## Result: ISSUES REMAIN")
        lines.append("Manual intervention may be required.")

    report_path.write_text("\n".join(lines), encoding="utf-8")
    return report_path


# ─── Main Doctor Loop ────────────────────────────────────────────────────────

def doctor_loop():
    """The main self-healing loop."""
    info("=" * 60)
    info("  OwnStack Doctor — AI-Powered Self-Healing IDE")
    info("=" * 60)
    print()

    # Validate configuration
    if PROVIDER == "anthropic" and not ANTHROPIC_API_KEY:
        error("ANTHROPIC_API_KEY not set. Set it or use DOCTOR_PROVIDER=openai")
        sys.exit(1)
    if PROVIDER == "openai" and not OPENAI_API_KEY:
        error("OPENAI_API_KEY not set.")
        sys.exit(1)

    info(f"Provider: {PROVIDER} | Model: {ANTHROPIC_MODEL if PROVIDER == 'anthropic' else OPENAI_MODEL}")
    print()

    # Phase 1: Initial build + tests
    info("Phase 1: Build & Test")
    skip_build = os.environ.get("DOCTOR_SKIP_BUILD", "")
    binary = BINARY if BINARY.exists() else BINARY_DEBUG
    if skip_build or binary.exists():
        info("Binary already exists, skipping build.")
        build_ok, build_errors = True, ""
    else:
        build_ok, build_errors = build_ide(release=False)
    if not build_ok:
        error("Initial build failed. Cannot proceed with visual testing.")
        error("Attempting to fix build errors with AI...")
        # Send build errors to AI for diagnosis
        diagnosis = analyze_build_errors(build_errors)
        if diagnosis and diagnosis.get("issues"):
            applied = apply_fixes(diagnosis["issues"])
            if applied > 0:
                info(f"Applied {applied} build fixes, retrying...")
                build_ok, _ = build_ide(release=False)

        if not build_ok:
            error("Build still failing after AI fix attempt. Manual intervention needed.")
            sys.exit(1)

    tests_ok, test_output = run_tests()
    print()

    # Phase 2: Launch IDE & Visual Loop
    info("Phase 2: Visual Analysis Loop")
    iterations_log = []
    ide_proc = None
    controller = None

    try:
        for iteration in range(1, MAX_ITERATIONS + 1):
            info(f"--- Iteration {iteration}/{MAX_ITERATIONS} ---")

            # Launch IDE if not running
            if ide_proc is None or ide_proc.poll() is not None:
                ide_proc, controller = launch_ide(ROOT)
                if controller is None:
                    error("Failed to launch IDE. Exiting.")
                    break
                time.sleep(2)  # Let the UI settle

            # Wait for idle
            controller.wait_idle(IDLE_TIMEOUT_MS)
            time.sleep(1)

            # Get state & diagnostics
            state = controller.get_state()
            diagnostics = controller.get_diagnostics()

            # Take screenshot
            screenshot_path = take_screenshot(controller, iteration)
            if not screenshot_path:
                warn("Could not capture screenshot, skipping visual analysis")
                iterations_log.append({
                    "status": "screenshot_failed",
                    "summary": "Screenshot capture failed",
                })
                continue

            # Analyze with AI
            context = ""
            if iterations_log:
                prev = iterations_log[-1]
                context = f"Previous iteration: {prev.get('summary', 'N/A')}"

            info("Analyzing screenshot with AI...")
            analysis = analyze_screenshot(screenshot_path, state, diagnostics, context)

            log_entry = {
                "status": analysis.get("status", "unknown"),
                "summary": analysis.get("summary", ""),
                "issues": analysis.get("issues", []),
                "screenshot": str(screenshot_path),
            }

            # If healthy, we're done
            if analysis.get("status") == "healthy":
                success("IDE looks healthy! No issues found.")
                log_entry["build_ok"] = build_ok
                log_entry["tests_ok"] = tests_ok
                iterations_log.append(log_entry)
                break

            # Apply fixes
            issues = analysis.get("issues", [])
            info(f"Found {len(issues)} issue(s):")
            for issue in issues:
                warn(f"  [{issue.get('severity', '?')}] {issue.get('issue', '?')}")

            if issues:
                # Kill IDE before modifying source
                info("Stopping IDE for code changes...")
                if ide_proc:
                    ide_proc.kill()
                    ide_proc.wait()
                    ide_proc = None

                applied = apply_fixes(issues)
                log_entry["fixes_applied"] = applied

                if applied > 0:
                    # Rebuild
                    build_ok, _ = build_ide(release=False)
                    log_entry["build_ok"] = build_ok

                    if build_ok:
                        tests_ok, _ = run_tests()
                        log_entry["tests_ok"] = tests_ok
                    else:
                        warn("Build failed after fixes — will retry next iteration")
                else:
                    info("No fixes could be applied, stopping.")
                    iterations_log.append(log_entry)
                    break

            iterations_log.append(log_entry)
            print()

    finally:
        # Cleanup
        if ide_proc and ide_proc.poll() is None:
            info("Shutting down IDE...")
            ide_proc.kill()
            ide_proc.wait()

    # Phase 3: Report
    print()
    info("Phase 3: Report")
    report_path = generate_report(iterations_log)
    success(f"Report saved: {report_path}")

    # Final summary
    print()
    info("=" * 60)
    total_issues = sum(len(l.get("issues", [])) for l in iterations_log)
    total_fixes = sum(l.get("fixes_applied", 0) for l in iterations_log)
    final_status = iterations_log[-1].get("status", "unknown") if iterations_log else "no_iterations"

    if final_status == "healthy":
        success(f"  RESULT: HEALTHY after {len(iterations_log)} iteration(s)")
    else:
        warn(f"  RESULT: {total_issues} issues found, {total_fixes} fixes applied")

    info(f"  Screenshots: {SCREENSHOT_DIR}")
    info(f"  Report: {report_path}")
    info("=" * 60)

    return 0 if final_status == "healthy" else 1


def analyze_build_errors(build_output):
    """Send build errors to AI for diagnosis (text-only, no screenshot)."""
    if PROVIDER == "anthropic":
        import urllib.request
        body = json.dumps({
            "model": ANTHROPIC_MODEL,
            "max_tokens": 4096,
            "messages": [{
                "role": "user",
                "content": f"""You are OwnStack Doctor. The IDE build failed with these errors:

```
{build_output[:8000]}
```

The project root is an IDE built in Rust with Floem UI framework.
Provide fixes as JSON:
{{
  "status": "issues_found",
  "summary": "build error summary",
  "issues": [
    {{
      "issue": "description",
      "severity": "critical",
      "file": "relative/path.rs",
      "fix": {{"old_text": "exact broken code", "new_text": "fixed code"}}
    }}
  ]
}}""",
            }],
        }).encode()

        api_base = os.environ.get("ANTHROPIC_BASE_URL", "https://api.anthropic.com")
        api_url = f"{api_base.rstrip('/')}/v1/messages"

        req = urllib.request.Request(
            api_url,
            data=body,
            headers={
                "x-api-key": ANTHROPIC_API_KEY,
                "anthropic-version": "2023-06-01",
                "content-type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                data = json.loads(resp.read())
                text = data["content"][0]["text"]
                json_match = re.search(r'\{[\s\S]*\}', text)
                if json_match:
                    return json.loads(json_match.group())
        except Exception as e:
            error(f"Build error analysis failed: {e}")
    return None


if __name__ == "__main__":
    sys.exit(doctor_loop())
