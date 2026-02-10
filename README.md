<h1 align="center">
  <a href="https://github.com/psykoniz/Ownstack" target="_blank">
  <img src="extra/images/logo.png" width=200 height=200/><br>
  OwnStack IDE
  </a>
</h1>

<h4 align="center">AI-Powered Native Code Editor — Built on Lapce</h4>

<div align="center">
  <a href="https://github.com/psykoniz/Ownstacklapce/actions/workflows/ci.yml" target="_blank">
    <img src="https://github.com/psykoniz/Ownstacklapce/actions/workflows/ci.yml/badge.svg" />
  </a>
  <a href="https://discord.gg/n8tGJ6Rn6D" target="_blank">
    <img src="https://img.shields.io/discord/946858761413328946?logo=discord" />
  </a>
</div>
<br/>

OwnStack IDE is a **native, GPU-accelerated code editor** with integrated AI agents for secure code assistance. Built as a fork of [Lapce](https://github.com/lapce/lapce), it combines lightning-fast performance with powerful AI capabilities.

![](https://github.com/lapce/lapce/blob/master/extra/images/screenshot.png?raw=true)

## ✨ Key Features

### Inherited from Lapce
- **Lightning Fast** — Written in pure Rust with [wgpu](https://github.com/gfx-rs/wgpu) GPU rendering
- **Native UI** — Built with [Floem](https://github.com/lapce/floem), no Electron overhead
- **Built-in LSP** — Intelligent code completion, diagnostics, and actions
- **Modal Editing** — First-class Vim-like editing (toggleable)
- **Remote Development** — VSCode-style remote development support
- **WASI Plugins** — Extensible via C, Rust, or AssemblyScript plugins
- **Built-in Terminal** — Integrated terminal for workspace commands

### OwnStack AI Features (Coming Soon)
- **Secure AI Agents** — Policy-controlled command execution
- **Sandboxed Operations** — All AI actions run in isolated environments
- **Full Audit Trail** — Every AI action is logged and traceable
- **Multi-Provider Support** — OpenRouter, Anthropic, Ollama, and more

## 🛠 Architecture

OwnStack IDE follows a strict security-first architecture:

```
┌─────────────────────────────────────────────────────────┐
│                    OwnStack IDE                         │
├────────────┬────────────┬────────────┬─────────────────┤
│ lapce-app  │ lapce-core │ lapce-proxy│ lapce-rpc       │
│ (Floem UI) │ (Xi Rope)  │ (LSP/Files)│ (Protocol)      │
├────────────┴────────────┴────────────┴─────────────────┤
│                  OwnStack Engine                        │
│  PolicyEngine │ PathValidator │ Sandbox │ AuditLogger  │
├─────────────────────────────────────────────────────────┤
│                  OwnStack Agent                         │
│      LLM Providers │ Toolkits │ Multi-Agent            │
└─────────────────────────────────────────────────────────┘
```

## 📦 Installation

### Pre-built Releases
Coming soon — see [Releases](https://github.com/psykoniz/Ownstacklapce/releases)

### Building from Source

**Prerequisites:**
- Rust 1.87.0+
- Platform-specific dependencies (see below)

```bash
# Clone the repository
git clone https://github.com/psykoniz/Ownstacklapce.git
cd Ownstacklapce

# Build release binary
cargo build --release

# Run
./target/release/ownstack-ide
```

**Ubuntu/Debian dependencies:**
```bash
sudo apt install libgtk-3-dev libxkbcommon-dev
```

**macOS:**
```bash
xcode-select --install
```

**Windows:**
Visual Studio Build Tools with C++ workload

## 📚 Documentation

- **[Architecture Guide](docs/ARCHITECTURE.md)** — System design and components
- **[Agent Directives](GEMINI.md)** — AI agent behavior rules
- **[Building Guide](docs/building-from-source.md)** — Detailed build instructions

## 🔒 Security

OwnStack IDE implements multiple security layers:

| Layer | Description |
|-------|-------------|
| **PolicyEngine** | Blocks dangerous commands (rm -rf, sudo, etc.) |
| **PathValidator** | Restricts file access to workspace only |
| **Sandbox** | Isolated execution with no network access |
| **AuditLogger** | Complete action history in JSONL format |

See [GEMINI.md](GEMINI.md) for complete security specifications.

## 🤝 Contributing

We welcome contributions! Please read:
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — Contribution guidelines
- [`GEMINI.md`](GEMINI.md) — Code standards and architecture rules

## 📄 License

OwnStack IDE uses a dual-license structure:

| Component | License |
|-----------|---------|
| Lapce core (lapce-*) | Apache 2.0 |
| OwnStack components (ownstack-*) | MIT |

See [LICENSE](LICENSE) (Apache 2.0) and [LICENSE-OWNSTACK](LICENSE-OWNSTACK) (MIT).

## 🙏 Acknowledgments

OwnStack IDE is built on the excellent work of:
- **[Lapce](https://lapce.dev)** — The lightning-fast code editor
- **[Floem](https://github.com/lapce/floem)** — Native Rust UI framework
- **[Xi-Editor](https://xi-editor.io)** — Rope science for text editing
- **[wgpu](https://wgpu.rs)** — GPU rendering

---

<div align="center">
  <sub>Built with ❤️ by the OwnStack team • Based on Lapce</sub>
</div>
