# Security Policy

## Reporting

Email **michael.groberman@icloud.com** with subject `[linux-mcp security]`, or open a [private vulnerability advisory](https://github.com/MichaelAdamGroberman/linux-mcp/security/advisories/new).

## Threat model

`linux-mcp` is a local MCP server speaking stdio JSON-RPC to a single MCP client (Claude Code, Cline, etc.). Trust boundary:

- **Trusted:** the MCP client. The server validates schemas and enforces hard limits on every call.
- **Untrusted (defense in depth):** anything reading the audit log, anything reading `/proc/<pid>/`, the helper binaries themselves (we shell out to a curated list — `mdfind`/`xdotool`/`wmctrl`/`ydotool`/`wtype`/`grim`/`scrot`/`maim`/`xclip`/`xsel`/`wl-clipboard`/`notify-send`/`zenity`/`kdialog` — and these are upstream-maintained).

## Concrete defenses

- **No remote attack surface.** stdio only. No listening sockets. No outbound network calls.
- **Filesystem allow-list with symlink resolution.** Every `fs_*` tool canonicalises through `std::fs::canonicalize` (or write-mode parent-only canonicalisation when the leaf doesn't exist yet) before checking allow + deny roots. This is the explicit defense against the symlink-bypass that [Desktop Commander's FAQ](https://github.com/wonderwhy-er/DesktopCommanderMCP/blob/main/FAQ.md) admits its allow-list does not prevent.
- **Default deny on system paths.** `/proc`, `/sys`, `/dev`, `/boot`, `/etc`, `/var/lib`, `/var/log`, `/run`, `/usr/sbin`, `/sbin`, `/root` are denied by default and override any allow-listed parent.
- **Process exec is opt-in by binary basename.** `LINUX_MCP_PROCESS_ALLOW` is empty by default → every `process_*` call is refused. To use it, the operator explicitly lists basenames (`git`, `rg`, etc.). Shell mode (`shell=true`) requires `/bin/sh` to be in that list.
- **No `run_shell` / `eval_*` escape hatches.** Every tool is a named typed function with bounded inputs.
- **Hard caps:** `fs_read` 10 MB, `fs_write` 50 MB, `fs_list` 5,000 entries, `process_run` 1 MB stdout / 256 KB stderr / 30 s default (300 s max) timeout, `type_text` 10,000 chars, `wait_ms` 60,000 ms.
- **`process_kill` cross-user guard.** Refuses pid 1 unconditionally and refuses any pid owned by a different uid unless `LINUX_MCP_PROCESS_KILL_ANY=1` is set (acknowledging the operator's intent).
- **Audit log.** Every tool call writes a JSONL line (timestamp, level, msg, meta) to `$XDG_STATE_HOME/linux-mcp/audit.log`, rotated at 10 MB. No telemetry.

## Out of scope

- Kernel-level privilege escalation, SELinux/AppArmor policy bugs, container escape — these are upstream concerns.
- Bugs in helper binaries (`xdotool` etc.) — report upstream.
- Side-channel disclosures via screenshot / clipboard / pasteboard contents — these are user-controlled.

## Hardening checklist (release)

- [ ] Built with `--release` and `lto = "thin"` (in `Cargo.toml`)
- [ ] No `unsafe` blocks added (run `rg unsafe` before tagging)
- [ ] `cargo audit` clean
- [ ] `cargo deny check` clean
- [ ] Stripped symbols in release binary
