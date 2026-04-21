# engram-mcp

Rust implementation of EngramMcp.

`engram-mcp` gives agents a small persistent memory that keeps what continues to matter and lets the rest fade away.

Persistent memory for Model Context Protocol agents.

It is built for continuity across sessions.
Not perfect recall. Not a transcript.
Just the context an agent should still have next time: preferences, durable facts, useful lessons, and work state worth carrying forward.

## Configuration

By default, `engram-mcp` stores memory in `.engram/memory.json` under the current working directory.

Startup options:

- `--file <path>` stores memory at a fixed location

Use an absolute path for `--file` when you want the memory location to stay stable across launches.

## Tools

- `recall`
- `remember_short`
- `remember_medium`
- `remember_long`
- `reinforce`
- `forget`
