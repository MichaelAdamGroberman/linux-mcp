# linux-mcp

[![release](https://img.shields.io/github/v/release/MichaelAdamGroberman/linux-mcp?display_name=tag&sort=semver)](https://github.com/MichaelAdamGroberman/linux-mcp/releases)
[![Linux](https://img.shields.io/badge/Linux-x86__64%20%2B%20arm64-blue.svg)](https://github.com/MichaelAdamGroberman/linux-mcp)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![display](https://img.shields.io/badge/display-X11%20%2B%20Wayland-success)](https://github.com/MichaelAdamGroberman/linux-mcp#display-server-coverage)
[![tools](https://img.shields.io/badge/tools-32-informational)](https://github.com/MichaelAdamGroberman/linux-mcp#tools-32)
[![mcp](https://img.shields.io/badge/protocol-MCP%202025--06--18-purple)](https://modelcontextprotocol.io)

Native Linux control via the Model Context Protocol. Companion to [`mac-mcp`](https://github.com/MichaelAdamGroberman/mac-mcp). Stdio JSON-RPC, single static binary, typed allow-listed surface, runtime detection of X11 vs Wayland.

## Why

Linux MCP options today are either:
- General-purpose `run_shell` wrappers (e.g. Desktop Commander) — work on Linux but exactly the failure modes mac-mcp's README pushes back on (allow-list bypass via command substitution, no real sandbox).
- macOS-only or Windows-only tools that don't translate.

`linux-mcp` ships the same typed-and-allow-listed design as mac-mcp, on Linux:

| | DC on Linux | linux-mcp |
|---|---|---|
| Tool surface | one giant `start_process` | 32 typed tools |
| Filesystem allow-list | advisory, bypassable | symlink-resolved, deny-first |
| Process exec | unrestricted | empty allow-list by default |
| Display server | n/a (text-only) | X11 + Wayland with runtime detect |
| Telemetry | partial opt-out | none |

## Tools (32)

| Bucket | Count | Tools |
|---|---:|---|
| Filesystem | 13 | `fs_read`, `fs_read_many`, `fs_write`, `fs_edit`, `fs_list`, `fs_stat`, `fs_copy`, `fs_move`, `fs_make_dir`, `fs_delete`, `fs_watch_once`, `fs_xattr_get`, `fs_xattr_set` |
| Process | 7 | `process_run`, `process_start`, `process_read_output`, `process_write_input`, `process_terminate`, `process_list`, `process_kill` |
| Input | 5 | `mouse_move`, `mouse_click`, `mouse_scroll`, `key_press`, `type_text` |
| Clipboard | 2 | `clipboard_read`, `clipboard_write` |
| Screenshots | 2 | `screenshot_screen`, `screenshot_window` |
| Notify / dialog | 2 | `notify`, `prompt_user` |
| Windows | 4 | `list_windows`, `focus_window`, `move_window`, `resize_window` |
| Util | 1 | `wait_ms` |

`fs_write_pdf` (mac-mcp's 14th fs tool) is reserved for v0.2.0; PDF generation on Linux without a heavy dep needs a deliberate choice.

## Display-server coverage

| Backend | Detection | Helpers required |
|---|---|---|
| Wayland | `$WAYLAND_DISPLAY` set | `wl-clipboard`, `grim`, `wtype` or `ydotool`, `swaymsg` (sway) or `hyprctl` (hyprland) for window IPC |
| X11 | `$DISPLAY` set, `$WAYLAND_DISPLAY` not set | `xclip` or `xsel`, `scrot`/`maim`/`import`, `xdotool`, `wmctrl` |
| None (headless) | neither set | filesystem + process + util tools still work |

Helper-binary tools fail with a clear `helper_missing` error pointing at the install command, never silently no-op.

## Install

### Ubuntu/Debian (.deb)
```bash
curl -L https://github.com/MichaelAdamGroberman/linux-mcp/releases/latest/download/linux-mcp_amd64.deb -o /tmp/linux-mcp.deb
sudo dpkg -i /tmp/linux-mcp.deb
```

### Fedora/RHEL (.rpm)
```bash
curl -L https://github.com/MichaelAdamGroberman/linux-mcp/releases/latest/download/linux-mcp.x86_64.rpm -o /tmp/linux-mcp.rpm
sudo dnf install /tmp/linux-mcp.rpm
```

### Static binary tarball (any distro, including Alpine/musl)
```bash
curl -L https://github.com/MichaelAdamGroberman/linux-mcp/releases/latest/download/linux-mcp-x86_64-musl.tar.gz | tar xz -C ~/.local/bin/
```

## Use with Claude Code (CLI)

Claude Desktop isn't on Linux, so the primary client is **Claude Code**:

```bash
# Direct MCP server registration
claude mcp add linux-control linux-mcp

# Or via the Claude Code plugin marketplace (also installs skills if/when added)
claude plugin marketplace add MichaelAdamGroberman/linux-mcp
claude plugin install linux-mcp
```

For other MCP clients (Cline, Continue, Zed, custom), point them at the `linux-mcp` binary over stdio.

## Configuration

All policy is environment-variable driven:

| Var | Default | Purpose |
|---|---|---|
| `LINUX_MCP_FS_ALLOW` | `$HOME` | colon-separated allow roots for `fs_*` |
| `LINUX_MCP_FS_DENY_EXTRA` | (empty) | additional deny roots beyond the built-in `/proc:/sys:/dev:/boot:/etc:/var/lib:/var/log:/run:/usr/sbin:/sbin:/root` |
| `LINUX_MCP_PROCESS_ALLOW` | (empty → refuses everything) | colon-separated executable basenames `process_*` may launch |
| `LINUX_MCP_PROCESS_KILL_ANY` | `0` | set `1` to allow `process_kill` to signal cross-user processes |
| `LINUX_MCP_LOG_LEVEL` | `info` | audit log level: `debug`/`info`/`warn`/`error` |

Audit log: `$XDG_STATE_HOME/linux-mcp/audit.log` (defaults to `~/.local/state/linux-mcp/audit.log`), JSONL, rotated at 10 MB.

## Build from source

```bash
git clone https://github.com/MichaelAdamGroberman/linux-mcp
cd linux-mcp
cargo build --release
./target/release/linux-mcp --help    # (no flags — pure stdio)
```

## License

MIT — see [LICENSE](LICENSE).

## Maintainer

Maintained by **Michael Adam Groberman**.

- **GitHub:** [@MichaelAdamGroberman](https://github.com/MichaelAdamGroberman)
- **LinkedIn:** [michael-adam-groberman](https://www.linkedin.com/in/michael-adam-groberman/)
- **Companion (macOS):** [mac-mcp](https://github.com/MichaelAdamGroberman/mac-mcp)

For security reports use GitHub private vulnerability advisories (see [SECURITY.md](SECURITY.md)) — **do not** use LinkedIn DMs for sensitive disclosures.
