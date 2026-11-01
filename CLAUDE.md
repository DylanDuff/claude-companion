# claude-companion

macOS background app: ⌥Space toggles a floating GPUI window; each message runs
`claude -p` and streams the reply in. See `docs/architecture.md` for detail.

## Layout

- `src/main.rs`: app setup, window options, Dock hiding (`setActivationPolicy`
  via `objc`).
- `src/companion.rs`: root view. Owns `State`, the pending reply, show/hide.
- `src/claude.rs`: CLI flags, process plumbing, `stream-json` parsing.
- `src/state.rs`: `state.json` persistence and the 5-minute expiry rule.
- `src/text_input.rs`: adapted from gpui 0.2.2 `examples/input.rs`.
- `src/hotkey.rs`: `global-hotkey` registration, polled from a GPUI task.
- `src/message_bubble.rs`, `src/theme.rs`: presentation only.

## Invariants

- **GPUI stack:** official `gpui = 0.2.2` only. `gpui-component` ≥0.7 builds
  on `gpui-pre` (a third-party Zed snapshot) whose types don't mix with
  `gpui`, so don't add it.
- **Fixed CLI working dir** `~/Library/Application Support/claude-companion/workdir`:
  `--resume` only finds sessions created from the same directory.
- **Tool lockdown:** `--tools` and `--allowedTools` both `WebSearch,WebFetch`
  (the second pre-approves them, since `-p` has nobody to answer prompts),
  plus `--strict-mcp-config` to load no MCP servers. User settings and hooks
  from `~/.claude` still apply.
- **Prompt goes over stdin**, never argv, so text starting with `-` isn't a flag.
- **`last_activity` updates only when a reply finishes**; expiry is skipped
  while a reply is pending.
- **`session_id` comes from the `result` event**; the first message omits
  `--resume`.
- Dropping `Companion::reply_task` kills the child (`kill_on_drop`).

## Build

- `cargo run`. gpui uses `runtime_shaders`, so Xcode's Metal toolchain isn't
  needed (shaders compile at launch).
- Bundle: `cargo bundle --release` (cargo-bundle, not yet installed).
- `CLAUDE_COMPANION_CLI` overrides the `claude` binary path; otherwise the
  usual install locations are checked, because Finder-launched apps lack the
  shell `PATH`.
