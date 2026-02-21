# hello_world WASI Plugin

This plugin implements the minimal OwnStack WASI JSON ABI:

- stdin (host -> plugin):
  - `{"tool_name":"hello_world","args":{"name":"Alice"}}`
- stdout (plugin -> host):
  - `{"success":true,"output":"Hello, Alice!"}`

## Build

```bash
cargo build --release --target wasm32-wasip1
```

The resulting artifact is:

```text
target/wasm32-wasip1/release/hello_world.wasm
```
