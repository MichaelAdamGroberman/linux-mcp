# Contributing

External contributions welcome — this repo is public and MIT-licensed.

## Local dev

```bash
cargo build --workspace                   # debug
cargo test --workspace                    # unit tests (no display server needed)
cargo run -p linux-mcp                    # stdio MCP server (talks to your tty)
cargo clippy --workspace -- -D warnings   # lint
```

## Smoke test (local)

```bash
( printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"smoke","version":"0"},"capabilities":{}}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
  sleep 1
) | cargo run -q -p linux-mcp 2>/dev/null | head -1
```

## New tools

- Must have a strict JSON schema with bounded inputs.
- Filesystem tools must use `linux_mcp_core::PathPolicy::check` before any I/O.
- Process tools must consult `linux_mcp_core::ProcessPolicy::is_allowed` for every executable basename.
- No `run_shell` / `eval_*` — see SECURITY.md.
- Add a unit test in `crates/linux-mcp-core/src/*.rs` (or a new `tests/` integration test) for any non-trivial logic.

## Style

- `rustfmt` defaults; `cargo fmt` before pushing.
- Pure functions live in `linux-mcp-core`; tools that need the rmcp model live in `crates/linux-mcp/src/tools/`.
