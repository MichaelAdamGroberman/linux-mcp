## Summary

- What this changes and why.

## Tools touched / added

- [ ] Lists new/changed tools and their schemas.

## Checklist

- [ ] No `run_shell` / `eval_*` introduced.
- [ ] All filesystem tools call `PathPolicy::check` before I/O.
- [ ] Process exec only via `ProcessPolicy::is_allowed`.
- [ ] Hard limits set on inputs (timeouts, result-size caps).
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` all pass.

## Risk

- New helper-binary dependency? (yes/no — name it)
- Cross-distro behavior change? (e.g. tested on Ubuntu but assumes systemd)
- Breaking change to existing tool surface? (yes/no)
