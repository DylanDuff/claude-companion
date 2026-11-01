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
3. Esc hides the window (or leaves the settings view). Losing key status
   hides it before a chat has started (no message and no reply streaming),
   always; after that, only with "Hide when clicking away" on
   (`hides_on_blur`). Either way, it only hides if focus is still
   elsewhere 150ms later (`BLUR_GRACE`), since key status can flicker while
   the panel is being shown.
4. On hide, an idle timer is armed. If the window is still hidden and no
   reply is running when it fires (after the session timeout), the `claude`
   process is killed.
5. ⌘N starts a new chat. ⌘Q quits. Once a chat has started, a **×**
   appears at the top left, in a bar above the chat. It clears the
   conversation and hides, so the next show starts fresh. The button is a
   solid circle with the × cut out (`close-filled.svg`, via an SVG mask),
   because gpui tints an icon in a single colour.

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
- Composer row: **+** attaches images, **sliders** open Settings (web
  access is switched there), and the **send** button (light when there's
  text or an image to send).
- **Image attachments** (`attachments.rs`): from **+** (gpui's file picker),
  ⌘V or dropping files onto the panel. PNG, JPEG, GIF and WebP up to 5 MB.
  - Each image is copied into `attachments/` in the support folder
    (pasted images are written there), so it outlives the original or the
    clipboard, shows after a relaunch, and can be re-sent by Retry.
    `Message.images` holds the paths, and `State::clear` (new chat, expiry)
    deletes them.
  - Sending turns them into base64 `image` content blocks ahead of the text
    block (`Process::send`). Text-only messages stay a plain string.
  - The composer shows removable thumbnails, and a red notice line for a
    rejected file. The transcript shows sent images as 120pt squares above
    the bubble. Fixed-size images are a clipping wrapper with the `img` at
    `size_full()` + `ObjectFit::Cover`, because an `img` sized directly
    keeps its aspect ratio.
  - The picker opens without activating the app, so `pick_images` calls
    `Panel::activate_app` (only here; showing the panel never activates)
    and sets `picking` so hide-on-blur ignores the focus loss. It re-shows
    the panel when the picker closes.
  - ⌘V goes to the input first. With no text on the clipboard it calls
    `propagate()`, and the root's `paste_image` takes the image entry.
- **Corner radii nest** (`theme.rs` geometry constants): each radius is the
  radius inside it plus the gap between them. The chain starts at the
  composer card, whose radius is chosen: `COMPOSER_RADIUS` = 18. (The 36pt
  round buttons inside it, 6pt in, would need 24 to be concentric too,
  which reads as too round.) Panel = 18 + `COMPOSER_MARGIN` (8) = 26. The
  compact bar is the card on its own at the card's scale: same 6pt inset,
  and the panel's radius switches to `COMPACT_RADIUS` (18) while compact
  (`corner_radius`, applied natively by `Panel::set_corner_radius` in the
  same task as the resize). `COMPACT_HEIGHT` (84) is derived from the same
  constants and grows for thumbnails and the notice line. The close button
  follows the rule too: its 20pt circle sits `CLOSE_INSET` = 26 − 10 = 16pt
  from the top and left, concentric with the panel's corner, and its bar is
  16 + 20 + 16 tall.
- **Bottom fade:** gpui has no gradient masks, so `render_chat_area`
  overlays a gradient from transparent to the panel's own tint over the
  chat's bottom `CHAT_BOTTOM_PADDING`. Text dims as it scrolls under the
  composer, while text at rest (above the padding) is untouched.
- **The chat clips at the composer's top edge:** the gap above the card is
  the chat's bottom padding, not the card's top margin, so messages scroll
  right down to the card.
- Under each reply: **Copy** (raw Markdown; shows a tick for 1.5s),
  **Read aloud** (`/usr/bin/say -f -` fed the document's
  `plain_text()`, code blocks skipped; click again to stop, as dropping the
  task kills `say`) and, on the latest reply when idle, **Retry**.
  Retry drops the reply and re-sends the question. The CLI session can't be
  rewound, so Claude still has the earlier answer in context.
- **Selecting text** (`selection.rs`): gpui 0.2.2 has no selectable text,
  so every piece of transcript text (paragraphs, list items, table cells,
  code, user and error messages) goes through `Selectable::text`, which
  builds it from text runs, paints the selected part with a background, and
  registers its `TextLayout` in the frame's `TextRegistry`. Positions are
  `TextPoint { message, offset }`, where pieces of a message take
  consecutive offsets in drawing order, so a selection can span pieces and
  messages. On mouse-down the root hit-tests the registry: on text, a
  selection starts and the press never moves the window; on empty space,
  the selection clears and a window drag may start. While dragging,
  `nearest` resolves gaps between pieces. ⌘C is the input's `Copy` action:
  the input copies its own selection, or calls `propagate()` so the root
  copies the transcript selection (pieces joined by newlines). Selections
  are cleared on new chat, retry and expiry.
- **Auto-scroll while selecting:** while a selection drag is within
  `AUTOSCROLL_ZONE` (28pt) of the chat's top or bottom edge, or past it
  (e.g. over the composer), a 16ms loop scrolls the chat (faster the deeper
  in, up to 24pt a step) and re-extends the selection. gpui only sends mouse
  moves while the pointer is over the panel, and no mouse-up for a release
  outside it, so the loop checks `Panel::left_button_down`
  (`NSEvent.pressedMouseButtons`) and stops when the button is up.
- **Moving the panel:** press anywhere away from text and drag. After `DRAG_THRESHOLD`
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
{ "session_id": "…", "last_activity": 1793500000,
  "transcript": [{ "role": "user", "text": "…", "images": ["…/attachments/1793…-0.png"] }] }
```

`images` is omitted when empty. Attached images live in `attachments/`.

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
  --strict-mcp-config --append-system-prompt <COMPANION_CONTEXT> \
  [--model <m>] [--effort <e>] [--resume <session_id>]
```

`--append-system-prompt` adds `COMPANION_CONTEXT`. It tells Claude it is in a
small floating chat window rather than a terminal session: no shell, files
or project, only the tools it's given, and replies render as Markdown in a
narrow panel. Without it, Claude Code talks about the placeholder working
directory and offers to run commands. The text is fixed on purpose. The CLI
records the system prompt once per conversation (`--system-prompt-snapshot`
defaults on) and reuses it verbatim on resume, so anything that varies,
like the enabled tools, would go stale. The tool list itself arrives fresh
through `--tools` on every launch.

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
- **Highlighting:** syntect with its bundled grammars (pure-Rust
  `fancy-regex`) and a built-in **Catppuccin Macchiato** theme
  (`code_theme.rs`), matching the user's Zed theme. The colours were copied
  once from the Catppuccin Zed extension. Zed styles tree-sitter captures
  while syntect grammars emit TextMate scopes, so `SCOPES` pairs TextMate
  selectors with the colour Zed gives the matching capture. Italic comments
  and similar styles carry over. Where a grammar gives different tokens one
  scope, they share a colour (Rust's `let` and `i32` are both
  `storage.type.rust`).
- **Languages:** unknown languages render plain. `grammar_token` maps
  common fence labels the bundled grammars lack (`ts` → `js`, `zsh` → `sh`,
  …). The grammars load on a background thread at launch
  (`highlight::preload`).
- **Caching:** `DocumentCache` (owned by `Companion`) keys parsed documents
  by a hash of their text and drops entries unused in the last render, so a
  message is parsed and highlighted once. The streaming reply is re-parsed
  only when new text arrives.
- **Inline text** is built from explicit `TextRun`s, so inline code can
  switch to Menlo. Links are clickable (`InteractiveText` → `open_url`).
- **Code blocks** are a rounded card on the theme's background (`BASE`),
  with a header (language label and Copy) above a horizontal scroller (no
  wrapping, Menlo):
  - **Sticky header** (`sticky.rs`): gpui has no `position: sticky`, so
    `StickyHeader` lays the header out above the body and, in prepaint,
    shifts it down to the top of the visible chat (the content mask) while
    the block is partly scrolled away, stopping at the block's bottom. It
    shifts before the header's prepaint, so the Copy button's hitbox moves
    with it. The element paints the header's background itself, after the
    body. Its top corners are rounded to match the card in place and square
    once pinned, since rounded corners mid-card let scrolled code show
    through.
  - **Copied state:** `MarkdownView` is rebuilt every frame and holds no
    state, so `Companion` passes a `CodeCopy` (the copied block's key and an
    `on_copy` listener). Reply and code copies share one `Copied` state, and
    `flash_copied` shows the tick for 1.5s.
  - **Edge-to-edge scrolling:** the card has no padding. The code's padding
    is inside the scrolled content, so code rests inset but scrolls all the
    way to the card's edges. The code is a non-shrinking flex item;
    otherwise gpui's block layout sizes it to the container and leaves no
    scroll range.
  - **Scroll axes** (`scroll.rs`): gpui turns wheel movement along one axis
    into scrolling along the other for single-axis scrollers, and sends each
    wheel event to every scroller under the pointer. `lock_axis` is set on
    both the code scroller and the chat, and `contain_horizontal` stops
    mostly-sideways swipes at a code block, so swiping code doesn't also
    scroll the chat.

## Known limitations

- Selection is by drag only: no double-click word selection or shift-click
  extension. Images in replies show as their alt text. Large photos aren't
  downscaled, so images over 5 MB are refused.
- The input is single-line, and pasted newlines become spaces.
- Shortcuts come from a fixed list; there is no key recorder.
