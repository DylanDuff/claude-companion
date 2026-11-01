# claude-companion

macOS background app: ⌥Space toggles a floating GPUI window; each message runs
`claude -p` and streams the reply in. See `docs/architecture.md` for detail.

## Layout

- `src/main.rs`: app setup, window options, Dock hiding (`setActivationPolicy`
  via `objc`).
- `src/companion.rs`: root view. Owns `State`, `Settings`, the warm
  `claude` session, the pending reply, show/hide and the chat/settings view.
- `src/claude.rs`: CLI flags, the long-lived process (`stream-json` in and
  out), event parsing.
- `src/state.rs`: `state.json` (conversation) and the idle-expiry rule.
- `src/settings.rs`: `settings.json`; `choices!` declares each enum setting
  and its labels together. `src/storage.rs` handles shared file I/O.
- `src/settings_panel.rs`, `src/segmented.rs`, `src/switch.rs`: settings
  view (sections of `choice` rows with a `Segmented<T: Choice>` and `toggle`
  rows with a `Switch`), opened from the composer's sliders icon.
- `src/text_input.rs`: adapted from gpui 0.2.2 `examples/input.rs`.
- `src/hotkey.rs`: `global-hotkey` registration, polled from a GPUI task.
- `src/status_item.rs`: the menu bar icon and its menu (AppKit), feeding
  commands to the companion over a channel.
- `src/markdown.rs` (parse to blocks, `DocumentCache`), `src/markdown_view.rs`
  (render), `src/highlight.rs` (syntect), `src/code_theme.rs` (built-in
  Catppuccin Macchiato), `src/sticky.rs` (pinned code headers),
  `src/scroll.rs` (scroll-axis helpers): assistant replies as Markdown.
- `src/panel.rs`: native NSPanel work (borderless rounded style, runtime
  corner radius, bottom-anchored resize, show/hide, drag, app activation for
  the file picker) and `Placement` (`window.json`).
- `src/attachments.rs`: image attachments copied into the support folder,
  and their base64 content blocks.
- `src/selection.rs`: transcript text selection (`Selectable`,
  `TextRegistry`). `src/icon.rs`: embedded SVG icons (`icons!`) and
  `IconButton`.
- `src/shimmer.rs`: the per-character shimmer on activity statuses.
- `src/message_bubble.rs`, `src/theme.rs`: presentation only. Theme colours
  are translucent because the window background is `Blurred`.

## Invariants

- **GPUI stack:** official `gpui = 0.2.2` only. `gpui-component` ≥0.7 builds
  on `gpui-pre` (a third-party Zed snapshot) whose types don't mix with
  `gpui`, so don't add it.
- **Fixed CLI working dir** `~/Library/Application Support/claude-companion/workdir`:
  `--resume` only finds sessions created from the same directory.
- **Tool lockdown:** `--tools` lists only the enabled web tools (`""` disables
  all), `--allowedTools` pre-approves them (nobody can answer prompts in
  `-p`), and `--strict-mcp-config` loads no MCP servers. User settings and
  hooks from `~/.claude` still apply.
- **One warm process per conversation:** started on show (prewarm), messages
  written to stdin as JSON lines. It is killed by ⌘N, on expiry, after the
  idle timeout while hidden, and on model, effort or tool changes (deferred
  until no reply is running; it resumes the same session).
- **Turn lifecycle:** a reply is committed and the input unlocked at the
  last `message_delta` stop (`end_turn` etc.). The `result` arrives
  about 1.8s later, after the CLI's `post_turn_summary`, and only settles
  `session_id`. A message sent meanwhile is queued by the CLI. A `result`
  with no turn running is a startup failure (bad `--resume`) and is ignored;
  `on_exit` reports it and drops the session id.
- **`last_activity` updates only when a reply finishes**; expiry is skipped
  while busy (`pending` or `unsettled`).
- **Native panel calls go through the stored `Panel`**, and resizes run in a
  spawned task (`cx.spawn`), never inside an update, `render` or
  `cx.defer`: gpui drops AppKit's synchronous resize report while the app is
  borrowed, and keeps drawing at the old size.
- **Show/hide go through `panel.rs`** (`makeKeyAndOrderFront:` /
  `orderOut:` on the non-activating NSPanel). Don't use `cx.activate` or
  `cx.hide`: activating the app races with gpui's key-window workaround.
  Hide-on-blur is debounced (`BLUR_GRACE`).
- **Markdown text uses explicit `TextRun`s** (`markdown_view::runs`) so
  inline code can switch to Menlo. Runs don't inherit colour or weight from
  parents, so pass them down as `Look`.
- **Corner radii are derived, not chosen** (`theme.rs`): change a size or
  inset and the radii follow. Don't hard-code a radius on the panel or the
  composer.
- **Scrollers lock their axis** (`scroll::lock_axis`), or gpui converts
  cross-axis wheel movement and nested scrollers fight.
- **The system prompt is fixed** (`claude::COMPANION_CONTEXT`): the CLI
  replays the recorded prompt on resume, so put nothing variable in it.
- **All transcript text goes through `Selectable::text`** so it can be
  hit-tested and highlighted; plain `StyledText` in the transcript won't be
  selectable. `TextRegistry` is rebuilt every render and only hit-tested
  between frames, when layouts have bounds.
- **Inline marks must stay non-overlapping** (`InlineBuilder::finish`):
  GPUI highlight runs can't overlap.
- `Session` fields are ordered reader-first so a deliberate kill cancels the
  reader before it can report the exit.

## Build

- `cargo run`. gpui uses `runtime_shaders`, so Xcode's Metal toolchain isn't
  needed (shaders compile at launch).
- Bundle: `scripts/build-dmg.sh` builds `dist/Claude Companion.app` and
  `dist/Claude-Companion-<version>.dmg` using only macOS/Xcode tools. The
  bundle identifier and minimum macOS live in that script; the version
  comes from `Cargo.toml`. The app is signed ad hoc.
- Icon: `assets/icon/AppIcon.png` (1024px), drawn by
  `swift scripts/make-icon.swift assets/icon/AppIcon.png`. Edit the script,
  not the PNG.
- `CLAUDE_COMPANION_CLI` overrides the `claude` binary path; otherwise the
  usual install locations are checked, because Finder-launched apps lack the
  shell `PATH`.
