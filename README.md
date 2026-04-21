# engram-mcp

Rust implementation of EngramMcp.

`engram-mcp` gives agents a small persistent memory that keeps what continues to matter and lets the rest fade away.

Persistent memory for Model Context Protocol agents.

It is built for continuity across sessions.
Not perfect recall. Not a transcript.
Just the context an agent should still have next time: preferences, durable facts, useful lessons, and work state worth carrying forward.

## Status

- functionally aligned with the current C# implementation
- built on `rmcp`, `tokio`, `serde`, `serde_json`
- tested against real MCP stdio handshake
- includes compatibility fixtures for C#-style memory files

## Installation

### From source

```bash
cargo install --git https://github.com/bob-clawd/engram-mcp engram-mcp
```

### Local build

```bash
cargo build --release
```

Binary:

```text
target/release/engram-mcp
```

## Configuration

By default, `engram-mcp` stores memory in `.engram/memory.json` under the current working directory.

Startup options:

- `--file <path>` stores memory at a fixed location

Use an absolute path for `--file` when you want the memory location to stay stable across launches.

Example:

```json
{
  "mcp": {
    "memory": {
      "type": "local",
      "command": [
        "engram-mcp",
        "--file",
        "/absolute/path/to/memory.json"
      ]
    }
  }
}
```

## Tools

- `recall`
- `remember_short`
- `remember_medium`
- `remember_long`
- `reinforce`
- `forget`

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
```

## Releases

- CI runs on Linux, macOS, Windows
- tagged releases `v*.*.*` build archives for Linux, macOS, Windows
- release artifacts are uploaded automatically by GitHub Actions

## License

Apache-2.0
