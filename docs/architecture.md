# Architecture

## Lifecycle

1. Launch: activation policy set to Accessory (no Dock icon or menu bar),
   settings loaded, icons registered (`icon::Assets`), the window opened as
   `WindowKind::PopUp` (440 wide, no title bar, blurred background),
   the native `Panel` configured, and the configured shortcut registered.
2. The shortcut calls `Companion::toggle`:
   - **visible and key** → `panel::hide` (`orderOut:`).
   - otherwise → apply the expiry rule, **start `claude` if it isn't
     running** (prewarm), `panel::show` (`makeKeyAndOrderFront:`), focus,
     scroll to the bottom.

   `Panel::configure` switches the panel to a borderless non-activating
   style mask, which removes the title bar, traffic lights and frame
   outline, and clips the content view and gpui's blur subview to
   `CORNER_RADIUS`. The root element draws the matching hairline border. The
   window uses `WindowBackgroundAppearance::Blurred`, and the theme colours
   are partly transparent so the blur shows through.

   The PopUp window is a non-activating `NSPanel`, so it takes keyboard
   focus while the previous app stays frontmost, and hiding returns focus to
   that app. The app itself is never activated or hidden: doing that
   (`cx.activate` / `cx.hide`) raced with gpui's key-window workaround and
   made the shortcut unreliable.
3. Esc hides the window (or leaves the settings view). With "Hide when
   clicking away" on, losing key status hides it, but only if focus is still
   elsewhere 150ms later (`BLUR_GRACE`), since key status can flicker while
   the panel is being shown.
4. On hide, an idle timer is armed. If the window is still hidden and no
   reply is running when it fires (after the session timeout), the `claude`
   process is killed.
5. ⌘N or **+** starts a new chat. ⌘Q quits.

## Layout (`companion.rs`)

- **Compact** (no conversation, chat view): the panel is just the composer,
  `COMPACT_HEIGHT` tall.
- **Expanded** (any message): transcript above, composer as an inset card
  below, `EXPANDED_HEIGHT` tall.
- **Settings**: `SETTINGS_HEIGHT` (720), tall enough to show every section
  without scrolling.
- `sync_height` runs on every notify (`observe_self`) and resizes only on
  change, from a spawned task holding the stored `Panel`. AppKit reports a
  resize to gpui synchronously, and gpui silently drops that report if the
  app is borrowed at that moment, which is true inside any update, `render`,
  `defer_in` or `cx.defer`. gpui then keeps laying out for the old size
  (e.g. the compact bar laid out 540pt tall in a 96pt window, its icon row
  cut off). A spawned task runs on the main queue outside the borrow, like
  gpui's own `Window::resize`. It is queued before the next display-link
  frame, so the view change and the new size are drawn together, as checked
  frame by frame in a 60fps screen recording.

  `Panel::set_height` keeps the **bottom** edge fixed (AppKit frames have a
  bottom-left origin, so it only changes the height), because the panel
  usually sits near the bottom of the screen and must grow upward. If the
  grown panel would extend past the top of the screen's `visibleFrame`, it
  slides down just enough to fit.
- **Focus:** in the chat view the root's mouse-down re-focuses the input and
  calls `prevent_default`, so clicking anywhere never takes keystrokes away
  from it. `Panel::show` also makes gpui's view first responder, because
  changing the style mask can leave the window itself as first responder,
  and then keystrokes never reach gpui.
- Composer row: **+** new chat, **globe** toggles web search and page
  reading together (blue when on; the same settings as the Settings page,
  restarting `claude` once idle), **sliders** open Settings, and the
  **send** button (light when there's text to send).
- Under each reply: **Copy** (raw Markdown; shows a tick for 1.5s),
  **Read aloud** (`/usr/bin/say -f -` fed the document's
  `plain_text()`, code blocks skipped; click again to stop, as dropping the
  task kills `say`) and, on the latest reply when idle, **Retry**.
  Retry drops the reply and re-sends the question. The CLI session can't be
  rewound, so Claude still has the earlier answer in context.
- **Moving the panel:** press anywhere and drag. After `DRAG_THRESHOLD`
  (4px) of movement with the left button held, `panel::begin_drag` hands the
  current event to `performWindowDragWithEvent:` and the window server moves
  the window until release. Waiting for movement keeps plain clicks working
  on buttons and links. Drags are skipped while the text input
  `is_selecting()`, so drags there select text. (Don't stop propagation in
  the input's mouse-down: gpui runs an element's listeners in reverse, so
  that also cancels the input's own click-to-focus.) gpui's `start_window_move` is a no-op
  on macOS, hence the native call.
- **Remembered position:** the panel's bottom-left corner (AppKit
  coordinates, since the bottom edge is what stays put) is saved to
  `window.json` 400ms after it stops moving (`observe_window_bounds`,
  debounced). At launch `Panel::restore` puts it back, but only if a
  connected screen's visible frame still contains it, so an unplugged
  display can't strand the panel off screen.
- Icons are hand-drawn 24×24 stroke SVGs in `assets/icons/`, compiled in by
  the `icons!` macro and rendered with `svg()` (tinted by text colour).

## Persistence (`storage.rs`)

Both files live in `~/Library/Application Support/claude-companion/` and are
written to `*.json.tmp` and then renamed over the original.

`state.json`, the conversation:

```json
{ "session_id": "…", "last_activity": 1793500000, "transcript": [{ "role": "user", "text": "…" }] }
```

`settings.json`: see [Settings](#settings). Fields missing from the file take
their default values (`#[serde(default)]`); a file that can't be parsed
falls back to all defaults.

## Session expiry (`state.rs`)

- On every show, unless a reply is running, the state resets if
  `now - last_activity ≥ session timeout`. A transcript with no
  `last_activity` (the app quit before the first reply) also counts as stale.
- `last_activity` is set when a reply's final message ends, so a long answer
  doesn't count against the timeout.

## CLI driver (`claude.rs`)

One long-lived process per conversation:

```
claude -p --verbose --input-format stream-json --output-format stream-json \
  --include-partial-messages --tools <enabled> [--allowedTools <enabled>] \
  --strict-mcp-config [--model <m>] [--effort <e>] [--resume <session_id>]
```

Each message is written to stdin as a single line:
`{"type":"user","message":{"role":"user","content":"…"}}`. The process
stays alive between turns. `std::process` is used for spawning, with stdout
and stderr wrapped in `smol::Unblock`. stdin is written synchronously, since
messages are tiny. Dropping `Process` kills and reaps the child.

Measured on CLI 2.1.316 (Sonnet 5.5, low effort):

| | Cold `claude -p` per message | Warm process |
| --- | --- | --- |
| Startup before the model call | ~1.0s | paid during prewarm |
| Send → first text | ~3.2s (Opus, default effort) | ~2.0s |
| Reply end → `result` (`post_turn_summary`) | ~1.8s | ~1.8s, but the input is already unlocked |

`post_turn_summary` is internal CLI work with no documented flag to turn it
off (`CLAUDE_CODE_POST_TURN_MEMORY=0` made no difference). A message sent
during it is queued and starts once it finishes.

### Events

| stream-json line | `Event` | Effect |
| --- | --- | --- |
| `stream_event` / `message_start` | `MessageStarted` | paragraph break between assistant messages (e.g. around a tool call) |
| `stream_event` / `content_block_delta` with `text_delta` | `TextDelta` | appended to the pending reply |
| `stream_event` / `content_block_start` with `tool_use` or `server_tool_use` | `ToolStarted` | status line ("Searching the web…") |
| `stream_event` / `message_delta` with a stop reason | `MessageEnded { last }` | `last` unless `tool_use` / `pause_turn`. On the last message: commit, unlock, mark unsettled |
| `result` | `Finished { session_id, outcome }` | saves `session_id`; on error, shows it. Ignored if no turn is running |

### Turn states (`companion.rs`)

```
idle ──send──▶ pending ──last MessageEnded──▶ unsettled ──result──▶ idle
                  └──────────── result (error before end) ────────────┘
```

`is_busy()` = pending or unsettled. It blocks expiry, idle shutdown and
restarts for settings changes. Sending is only blocked while pending.

If the process exits on its own:
- **while busy:** an error is shown and the session is dropped; the next
  message starts a new process with `--resume`.
- **while idle, with stderr output** (e.g. `No conversation found with
  session ID`): `session_id` is cleared and a notice says the next message
  starts a new conversation.

## Settings

Opened from the composer's sliders icon; the header's back chevron or Esc
returns. The panel edits a copy and emits `SettingsEvent::Changed`;
`Companion` saves and applies it.

Layout (`settings_panel.rs`): titled sections (Claude, Web, Window), each a
rounded card like the composer, with hairline dividers between rows and an
optional note underneath (muted, or red for a shortcut that couldn't be
registered). There are two kinds of row, both built by helpers that bind one
`Settings` field via `on_change`:

- `choice`: label above a full-width `Segmented<T: Choice>` (neutral track,
  equal-width segments, the selected one lifted).
- `toggle`: label and description on the left, a `Switch` on the right.

| Setting | Values (default **bold**) | Applies |
| --- | --- | --- |
| Model | Default (CLI's), Fable, Opus, **Sonnet**, Haiku | restarts `claude` with `--resume` once idle |
| Effort | Default, **Low**, Medium, High, XHigh, Max | same |
| Search the web / Read web pages | **On** / Off | same; both off passes `--tools ""` |
| Start a new chat after idle | 1, **5**, 10, 30 min, 1 hour | next show / idle timer |
| Shortcut | **⌥Space**, ⌃Space, ⌥⇧Space, ⌘⇧Space | at once. The new key is registered before the old one is released; on failure the old one stays and the panel shows why |
| Hide when clicking away | **On** / Off | at once |

To add a setting, add the field to `Settings` and its `Default` (enums via
`choices!`), then add a `Self::choice(…)` or `Self::toggle(…)` row to a
section in `settings_panel.rs`.

## Text input (`text_input.rs`)

A single-line field from gpui's `input.rs` example. It implements
`EntityInputHandler`, so IME, the character palette and selection work.
Additions: a `Submit` event on Enter (ignored while IME text is marked),
`take()`, ⌘←/⌘→, and horizontal scrolling (`scroll_x`) applied consistently
across painting, mouse hit-testing and IME bounds.

## Markdown (`markdown.rs`, `markdown_view.rs`, `highlight.rs`)

Assistant replies render as Markdown; user messages and errors stay plain.

- `markdown::parse` turns pulldown-cmark events (tables, strikethrough, task
  lists and GFM on) into a small model: `Block` = paragraph, heading, code,
  list, quote, table, rule; `Inline` = text plus marks and links. It is
  GPUI-free so it can be unit tested.
- Inline marks can nest (`**bold *both***`), but GPUI highlight runs must
  not overlap, so `InlineBuilder::finish` splits them at every boundary and
  merges the styles.
- Unclosed constructs (a code fence while a reply is still streaming) are
  closed in `Builder::finish`, so partial replies render sensibly.
- Code blocks are coloured by syntect (pure-Rust `fancy-regex`) using the
  **user's Zed theme** (`zed_theme.rs`), falling back to `base16-ocean.dark`:
  - The theme name comes from `~/.config/zed/settings.json` (JSONC: comments
    and trailing commas are stripped first). For `{ mode, light, dark }`, the
    dark theme is used, because the companion is always dark.
  - The theme is looked up by name in `~/.config/zed/themes/*.json` and
    `~/Library/Application Support/Zed/extensions/installed/*/themes/*.json`.
    Zed's built-in themes (One Dark etc.) are compiled into Zed, so they
    fall back.
  - Zed styles tree-sitter captures while syntect grammars emit TextMate
    scopes, so `SCOPES` maps TextMate selectors to Zed keys, each resolved
    with Zed's fallback (`keyword.import` → `keyword`). Colour, italic and
    bold carry over.
  - Code blocks use the theme's `editor.background`, so its colours sit on
    the surface they were designed for.
  - The theme loads once at launch; restart the companion after changing
    it in Zed.
  - Known gap: where a grammar gives different tokens one scope, they share
    a colour (Rust's `let` and `i32` are both `storage.type.rust`). Unknown languages render plain; `grammar_token` maps
  common fence labels the bundled grammars lack (`ts` → `js`, `zsh` → `sh`,
  …). The grammars load on a background thread at launch
  (`highlight::preload`).
- `DocumentCache` (owned by `Companion`) keys parsed documents by a hash of
  their text and drops entries unused in the last render, so a message is
  parsed and highlighted once. The streaming reply is re-parsed only when
  new text arrives.
- Rendering: code blocks have a language label, a Copy button and
  horizontal scrolling (no wrapping, Menlo). Two details make the scrolling
  work:
  - **Content width:** the code sits in a non-shrinking flex item inside a
    flex scroller. In gpui's default block layout the text was laid out
    exactly as wide as its container, so the scroll range was 0 and long
    lines were clipped.
  - **Axis lock:** `restrict_scroll_to_axis` is set on the scroller.
    Otherwise gpui turns vertical wheel movement over an x-only scroller
    into horizontal scrolling, and scrolling the chat past a code block
    would slide the code. Links are clickable
  (`InteractiveText` → `open_url`). Inline code can't change font family
  through a highlight, so it uses colour and a background instead.

## Known limitations

- Text in the transcript can't be selected or copied (code blocks have a
  Copy button). Images in replies show as their alt text.
- The input is single-line, and pasted newlines become spaces.
- Shortcuts come from a fixed list; there is no key recorder.
