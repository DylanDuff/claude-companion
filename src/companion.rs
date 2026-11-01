use std::{path::PathBuf, rc::Rc, time::Duration};

use gpui::{
    AnyElement, ClipboardItem, Context, Entity, ExternalPaths, FocusHandle, Focusable, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point,
    ScrollHandle, SharedString, Subscription, Task, Window, actions, div, img, prelude::*, px,
};
use smol::io::AsyncWriteExt;

use crate::{
    attachments,
    claude::{self, Event},
    hotkey,
    icon::{Icon, IconButton},
    markdown::DocumentCache,
    markdown_view::CodeCopy,
    message_bubble::MessageBubble,
    panel::{Panel, Placement},
    scroll,
    selection::{Selectable, Selection, TextRegistry},
    settings::Settings,
    settings_panel::{SettingsEvent, SettingsPanel},
    state::{Role, State},
    text_input::{self, Submit, TextInput},
    theme,
};

const CONTEXT: &str = "Companion";

/// How long focus must stay elsewhere before hide-on-blur hides the panel.
const BLUR_GRACE: Duration = Duration::from_millis(150);

/// Composer text sits this far inside the buttons' inset, lining up with
/// the icons drawn inside the buttons.
const TEXT_INSET: f32 = 8.;
const INPUT_LINE_HEIGHT: f32 = 22.;
const COMPOSER_ROW_GAP: f32 = 4.;
/// Panel height with no conversation: just the composer, plus the panel's
/// 1pt border top and bottom.
const COMPACT_HEIGHT: f32 = theme::COMPOSER_INSET * 2.
    + TEXT_INSET
    + INPUT_LINE_HEIGHT
    + COMPOSER_ROW_GAP
    + theme::BUTTON_SIZE
    + 2.;
/// Side of an attachment thumbnail in the composer.
const THUMBNAIL_SIZE: f32 = 52.;
/// Side of an attached image shown in the transcript.
const SENT_IMAGE_SIZE: f32 = 120.;
/// Height of the composer's notice line (e.g. an image too large to attach).
const NOTICE_HEIGHT: f32 = 18.;
/// Panel height once there is a conversation.
const EXPANDED_HEIGHT: f32 = 540.;
/// Panel height for settings, tall enough to show every section unscrolled.
const SETTINGS_HEIGHT: f32 = 720.;

/// How far the mouse must move while pressed before the panel starts moving,
/// so plain clicks still reach buttons.
const DRAG_THRESHOLD: f32 = 4.;

/// How long the panel must stay put before its position is saved.
const PLACEMENT_SAVE_DELAY: Duration = Duration::from_millis(400);

/// While drag-selecting, the chat scrolls when the pointer is this close to
/// its top or bottom edge (or past it).
const AUTOSCROLL_ZONE: f32 = 28.;
/// Fastest auto-scroll, in points per step.
const AUTOSCROLL_MAX_STEP: f32 = 24.;
const AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(16);

/// Height of the bar above the chat that holds the close button.
const TOP_BAR_HEIGHT: f32 = 40.;
/// Gap from the panel's left edge to the close button.
const CLOSE_INSET: f32 = 10.;

/// How long a Copy button shows its tick.
const COPIED_FEEDBACK: Duration = Duration::from_millis(1500);

actions!(companion, [Hide, NewChat]);

pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("escape", Hide, Some(CONTEXT)),
        KeyBinding::new("cmd-n", NewChat, Some(CONTEXT)),
    ]
}

/// A reply that is still streaming in.
#[derive(Default)]
struct Pending {
    reply: String,
    /// What Claude is doing when it isn't writing, e.g. searching.
    status: Option<SharedString>,
}

/// A running `claude` process plus the task reading its output. The reader
/// is declared first so it is cancelled before the process is killed and
/// never reports that deliberate exit.
struct Session {
    _reader: Task<()>,
    process: claude::Process,
}

/// Which Copy button shows its tick.
#[derive(Clone, PartialEq)]
enum Copied {
    /// A whole reply, by transcript index.
    Reply(usize),
    /// A code block, by its key from `MarkdownView`.
    Code(SharedString),
}

/// A reply being read aloud by `say`. Dropping it stops the speech.
struct Speech {
    /// Transcript index of the reply.
    index: usize,
    _task: Task<()>,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Chat,
    Settings,
}

pub struct Companion {
    state: State,
    settings: Settings,
    session: Option<Session>,
    pending: Option<Pending>,
    /// The last reply is in the transcript but its `result` hasn't arrived.
    /// Messages sent meanwhile are queued by the CLI.
    unsettled: bool,
    /// The reply ended without streamed text, so use the `result` text.
    needs_result_text: bool,
    /// Settings changed CLI flags while a reply was running.
    restart_when_idle: bool,
    /// Kills the process after the session timeout while hidden.
    idle_shutdown: Option<Task<()>>,
    /// Pending hide after the panel lost focus.
    blur_check: Option<Task<()>>,
    view: View,
    input: Entity<TextInput>,
    settings_panel: Entity<SettingsPanel>,
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    documents: DocumentCache,
    speech: Option<Speech>,
    /// The Copy button currently showing its tick.
    copied: Option<Copied>,
    copied_reset: Option<Task<()>>,
    /// The native panel, configured once at creation.
    panel: Option<Panel>,
    /// Saves the panel's position once it stops moving.
    placement_save: Option<Task<()>>,
    /// The panel height last requested, so it is only resized on change.
    height: Option<f32>,
    /// Images attached to the message being written.
    attachments: Vec<PathBuf>,
    /// Why the last attachment failed, shown in the composer.
    attach_notice: Option<SharedString>,
    /// The file picker is open: losing focus to it mustn't hide the panel.
    picking: bool,
    /// The corner radius last applied to the panel.
    radius: Option<f32>,
    /// Where the left button went down, while a window drag may still start.
    drag_origin: Option<Point<Pixels>>,
    /// Text drawn in the last frame, for hit-testing selections.
    text_registry: TextRegistry,
    selection: Option<Selection>,
    /// The left button went down on text and is extending `selection`.
    selecting: bool,
    /// Last pointer position while selecting, for auto-scroll.
    pointer: Point<Pixels>,
    /// Scrolls the chat while a selection is dragged to its edge.
    autoscroll: Option<Task<()>>,
    visible: bool,
    _subscriptions: Vec<Subscription>,
}

impl Companion {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Ask anything", cx));
        let settings_panel = cx.new(|_| SettingsPanel::new(settings.clone()));
        let panel = Panel::new(window);
        if let Some(panel) = panel {
            set_up_panel(panel);
        }
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &Submit, _, cx| this.submit(cx)),
            // Re-render as the text changes, so the send button follows it.
            cx.observe(&input, |_, _, cx| cx.notify()),
            // Every state change notifies; resize then, before the frame is
            // drawn, so the new layout never appears at the old size.
            cx.observe_self(|this, cx| this.sync_height(cx)),
            // Moves and resizes; remember where the user left the panel.
            cx.observe_window_bounds(window, |this, _, cx| this.schedule_placement_save(cx)),
            cx.subscribe_in(
                &settings_panel,
                window,
                |this, _, event, window, cx| match event {
                    SettingsEvent::Changed(settings) => this.apply_settings(settings.clone(), cx),
                    SettingsEvent::Closed => this.set_view(View::Chat, window, cx),
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && this.hides_on_blur() {
                    this.hide_after_blur(window, cx);
                }
            }),
        ];
        Self {
            state: State::load(),
            settings,
            session: None,
            pending: None,
            unsettled: false,
            needs_result_text: false,
            restart_when_idle: false,
            idle_shutdown: None,
            blur_check: None,
            view: View::Chat,
            input,
            settings_panel,
            focus_handle: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            documents: DocumentCache::default(),
            speech: None,
            copied: None,
            copied_reset: None,
            panel,
            placement_save: None,
            height: None,
            attachments: Vec::new(),
            attach_notice: None,
            picking: false,
            radius: None,
            drag_origin: None,
            text_registry: TextRegistry::default(),
            selection: None,
            selecting: false,
            pointer: Point::default(),
            autoscroll: None,
            visible: false,
            _subscriptions: subscriptions,
        }
    }

    /// A reply is streaming or hasn't settled yet.
    fn is_busy(&self) -> bool {
        self.pending.is_some() || self.unsettled
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible && window.is_window_active() {
            self.hide(window, cx);
        } else {
            self.show(window, cx);
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.idle_shutdown = None;
        // Never expire mid-reply: the reply resets the clock when it lands.
        let timeout = self.settings.session_timeout.duration();
        if !self.is_busy() && self.state.expire_if_stale(timeout) {
            self.selection = None;
            self.session = None;
            self.save();
        }
        // Start Claude while the user types, hiding its startup time.
        self.ensure_session(cx);
        self.visible = true;
        self.blur_check = None;
        if self.panel.is_none() {
            self.panel = Panel::new(window);
            if let Some(panel) = self.panel {
                set_up_panel(panel);
            }
        }
        if let Some(panel) = self.panel {
            panel.show();
        }
        self.focus_view(window, cx);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn hide(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.blur_check = None;
        if let Some(panel) = self.panel {
            panel.hide();
        }
        self.schedule_idle_shutdown(cx);
    }

    /// Hides once focus has stayed elsewhere briefly. Key status can flicker
    /// while the panel is being shown, and hiding on that would swallow the
    /// show.
    fn hide_after_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.blur_check = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(BLUR_GRACE).await;
            this.update_in(cx, |companion, window, cx| {
                if companion.visible && !companion.picking && !window.is_window_active() {
                    companion.hide(window, cx);
                }
            })
            .ok();
        }));
    }

    /// Saves the panel's position once it has stayed put briefly, so a drag
    /// writes the file once rather than on every step.
    fn schedule_placement_save(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panel else {
            return;
        };
        self.placement_save = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(PLACEMENT_SAVE_DELAY).await;
            panel.placement().save();
        }));
    }

    fn schedule_idle_shutdown(&mut self, cx: &mut Context<Self>) {
        let timeout = self.settings.session_timeout.duration();
        self.idle_shutdown = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(timeout).await;
            this.update(cx, |companion, _| {
                if !companion.visible && !companion.is_busy() {
                    companion.session = None;
                }
            })
            .ok();
        }));
    }

    fn set_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        self.view = view;
        self.focus_view(window, cx);
        cx.notify();
    }

    fn focus_view(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            View::Chat => window.focus(&self.input.read(cx).focus_handle(cx)),
            View::Settings => window.focus(&self.focus_handle),
        }
    }

    fn on_hide(&mut self, _: &Hide, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            View::Settings => self.set_view(View::Chat, window, cx),
            View::Chat => self.hide(window, cx),
        }
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = match self.view {
            View::Chat => View::Settings,
            View::Settings => View::Chat,
        };
        self.set_view(view, window, cx);
    }

    fn on_new_chat(&mut self, _: &NewChat, _: &mut Window, cx: &mut Context<Self>) {
        self.new_chat(cx);
    }

    fn new_chat(&mut self, cx: &mut Context<Self>) {
        self.speech = None;
        self.selection = None;
        self.session = None;
        self.pending = None;
        self.unsettled = false;
        self.needs_result_text = false;
        self.state.clear();
        self.save();
        self.ensure_session(cx);
        cx.notify();
    }

    fn apply_settings(&mut self, mut settings: Settings, cx: &mut Context<Self>) {
        let mut notice = None;
        if settings.hotkey != self.settings.hotkey
            && let Err(error) = hotkey::set(settings.hotkey, cx)
        {
            settings.hotkey = self.settings.hotkey;
            notice = Some(SharedString::from(error));
        }
        let restart = self.settings.changes_cli_flags(&settings);
        self.settings = settings;
        if let Err(error) = self.settings.save() {
            eprintln!("claude-companion: couldn't save settings: {error}");
        }
        if notice.is_some() {
            let settings = self.settings.clone();
            self.settings_panel
                .update(cx, |panel, cx| panel.reset(settings, notice, cx));
        }
        if restart {
            self.restart_when_idle = true;
            self.restart_if_idle(cx);
        }
    }

    /// Applies pending CLI flag changes by restarting Claude on the same
    /// conversation, once no reply is running.
    fn restart_if_idle(&mut self, cx: &mut Context<Self>) {
        if !self.restart_when_idle || self.is_busy() {
            return;
        }
        self.restart_when_idle = false;
        self.session = None;
        if self.visible {
            self.ensure_session(cx);
        }
    }

    /// Starts `claude` if it isn't running. Returns whether it is.
    fn ensure_session(&mut self, cx: &mut Context<Self>) -> bool {
        if self.session.is_some() {
            return true;
        }
        match claude::start(&self.settings, self.state.session_id.as_deref()) {
            Ok((process, output)) => {
                let reader = cx.spawn(async move |this, cx| {
                    let errors = output
                        .read(|event| {
                            this.update(cx, |companion, cx| companion.apply(event, cx))
                                .is_ok()
                        })
                        .await;
                    this.update(cx, |companion, cx| companion.on_exit(errors, cx))
                        .ok();
                });
                self.session = Some(Session {
                    _reader: reader,
                    process,
                });
                true
            }
            Err(error) => {
                self.state.push(Role::Error, error);
                self.save();
                cx.notify();
                false
            }
        }
    }

    fn can_send(&self, cx: &gpui::App) -> bool {
        self.pending.is_none()
            && (!self.attachments.is_empty() || !self.input.read(cx).text().trim().is_empty())
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_send(cx) {
            return;
        }
        let prompt = self.input.update(cx, |input, cx| input.take(cx));
        let prompt = prompt.trim();
        let images = std::mem::take(&mut self.attachments);
        self.attach_notice = None;
        self.state
            .push_with_images(Role::User, prompt, images.clone());
        self.save();
        self.dispatch(prompt, &images, cx);
    }

    /// Opens the file picker for images. The app is activated so the picker
    /// gets keyboard focus, and hide-on-blur is held off until it closes.
    fn pick_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picking = true;
        Panel::activate_app();
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let paths = picked.await.ok().and_then(Result::ok).flatten();
            this.update_in(cx, |companion, window, cx| {
                companion.picking = false;
                if let Some(paths) = paths {
                    companion.attach_files(&paths, cx);
                }
                companion.show(window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn attach_files(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.attach_notice = None;
        for path in paths {
            self.add_attachment(attachments::import_file(path));
        }
        cx.notify();
    }

    fn add_attachment(&mut self, imported: Result<PathBuf, String>) {
        match imported {
            Ok(path) => self.attachments.push(path),
            Err(reason) => self.attach_notice = Some(reason.into()),
        }
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            attachments::remove([self.attachments.remove(index)]);
            self.attach_notice = None;
            cx.notify();
        }
    }

    /// ⌘V with no text on the clipboard: attach a copied image instead. The
    /// input only passes ⌘V on when there's no text to paste.
    fn paste_image(&mut self, _: &text_input::Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let image = item.entries().iter().find_map(|entry| match entry {
            gpui::ClipboardEntry::Image(image) => Some(image),
            _ => None,
        });
        let Some(image) = image else {
            return;
        };
        let extension = match image.format {
            gpui::ImageFormat::Png => "png",
            gpui::ImageFormat::Jpeg => "jpeg",
            gpui::ImageFormat::Gif => "gif",
            gpui::ImageFormat::Webp => "webp",
            _ => {
                self.attach_notice =
                    Some("Only PNG, JPEG, GIF and WebP images can be attached.".into());
                cx.notify();
                return;
            }
        };
        self.attach_notice = None;
        self.add_attachment(attachments::import_bytes(&image.bytes, extension));
        cx.notify();
    }

    /// Asks again for the reply at `index`: drops it and re-sends the user
    /// message before it. The session still holds the earlier answer, so
    /// Claude sees this as a follow-up rather than a rewind.
    fn retry(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let transcript = &mut self.state.transcript;
        let Some(question) = transcript[..index]
            .iter()
            .rposition(|message| message.role == Role::User)
        else {
            return;
        };
        let prompt = transcript[question].text.clone();
        let images = transcript[question].images.clone();
        transcript.truncate(question + 1);
        self.selection = None;
        self.speech = None;
        self.save();
        self.dispatch(&prompt, &images, cx);
    }

    /// Sends `prompt` and `images` to the running session, starting one if
    /// needed.
    fn dispatch(&mut self, prompt: &str, images: &[PathBuf], cx: &mut Context<Self>) {
        if !self.ensure_session(cx) {
            return;
        }
        let session = self.session.as_mut().expect("session was just ensured");
        if let Err(error) = session.process.send(prompt, images) {
            self.session = None;
            self.state
                .push(Role::Error, format!("Couldn't reach claude: {error}"));
            self.save();
        } else {
            self.pending = Some(Pending::default());
        }
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::MessageStarted => {
                if let Some(pending) = self.pending.as_mut()
                    && !pending.reply.is_empty()
                    && !pending.reply.ends_with("\n\n")
                {
                    pending.reply.push_str("\n\n");
                }
            }
            Event::TextDelta(text) => {
                if let Some(pending) = self.pending.as_mut() {
                    pending.reply.push_str(&text);
                    pending.status = None;
                }
            }
            Event::ToolStarted(name) => {
                if let Some(pending) = self.pending.as_mut() {
                    pending.status = Some(tool_label(&name));
                }
            }
            Event::MessageEnded { last: false } => {}
            Event::MessageEnded { last: true } => {
                // Unlock now rather than at `result`, which follows a second
                // or two of post-turn work in the CLI.
                if let Some(pending) = self.pending.take() {
                    self.needs_result_text = !self.commit(pending.reply);
                    self.unsettled = true;
                }
            }
            Event::Finished {
                session_id,
                outcome,
            } => self.settle(session_id, outcome),
        }
        self.restart_if_idle(cx);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Adds a finished reply to the transcript and resets the idle clock.
    /// Returns false, adding nothing, when the reply is empty.
    fn commit(&mut self, reply: String) -> bool {
        let reply = reply.trim();
        if reply.is_empty() {
            return false;
        }
        self.state.push(Role::Assistant, reply);
        self.state.touch();
        self.save();
        true
    }

    fn settle(&mut self, session_id: Option<String>, outcome: Result<String, String>) {
        // A result with no turn running is a startup failure such as a bad
        // --resume. The process exits next and `on_exit` reports it.
        if !self.is_busy() {
            return;
        }
        if session_id.is_some() {
            self.state.session_id = session_id;
        }
        // Normally the reply was committed at its end; if the turn failed
        // before that, the streamed text is still pending.
        let streamed = if self.unsettled {
            self.unsettled = false;
            String::new()
        } else {
            self.pending
                .take()
                .map(|pending| pending.reply)
                .unwrap_or_default()
        };
        let needs_result_text = std::mem::take(&mut self.needs_result_text);
        match outcome {
            Ok(text) => {
                if !self.commit(streamed) && needs_result_text {
                    self.commit(text);
                }
            }
            Err(reason) => {
                self.commit(streamed);
                self.state.push(Role::Error, reason);
            }
        }
        self.save();
    }

    /// The process exited without being asked to.
    fn on_exit(&mut self, errors: String, cx: &mut Context<Self>) {
        self.session = None;
        let interrupted = self.is_busy();
        self.pending = None;
        self.unsettled = false;
        self.needs_result_text = false;
        if !interrupted && errors.is_empty() {
            return;
        }
        let reason = if errors.is_empty() {
            "claude exited before finishing the reply".to_string()
        } else {
            errors
        };
        // An unusable --resume (e.g. a deleted session) would fail the same
        // way every time, so start the next message fresh instead.
        if !interrupted && self.state.session_id.take().is_some() {
            self.state.push(
                Role::Error,
                format!("Couldn't resume the conversation, so the next message starts a new one.\n{reason}"),
            );
        } else {
            self.state.push(Role::Error, reason);
        }
        self.save();
        cx.notify();
    }

    fn save(&self) {
        if let Err(error) = self.state.save() {
            eprintln!("claude-companion: couldn't save state: {error}");
        }
    }

    /// ⌘C with a transcript selection. The input only passes ⌘C on when it
    /// has no selection of its own.
    fn copy_selection(&mut self, _: &text_input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.filter(|selection| !selection.is_empty()) else {
            return;
        };
        let text = self.text_registry.text(selection.range());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Copy state and handler for the code blocks in replies.
    fn code_copy(&self, cx: &mut Context<Self>) -> CodeCopy {
        CodeCopy {
            copied: match &self.copied {
                Some(Copied::Code(key)) => Some(key.clone()),
                _ => None,
            },
            on_copy: Rc::new(
                cx.listener(|this, (key, code): &(SharedString, String), _, cx| {
                    this.copy_code(key.clone(), code, cx)
                }),
            ),
        }
    }

    /// Discards the conversation and hides the panel, so the next show
    /// starts fresh with the compact bar.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_chat(cx);
        self.hide(window, cx);
    }

    /// Where message `index`'s text registers, with its part of the
    /// selection highlighted.
    fn selectable(&self, index: usize) -> Selectable {
        Selectable::new(
            index,
            self.text_registry.clone(),
            self.selection.map(|selection| selection.range()),
            theme::selection().into(),
        )
    }

    fn copy_reply(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(message) = self.state.transcript.get(index) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(message.text.clone()));
        self.flash_copied(Copied::Reply(index), cx);
    }

    fn copy_code(&mut self, key: SharedString, code: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
        self.flash_copied(Copied::Code(key), cx);
    }

    /// Shows `target`'s Copy button as copied for a moment.
    fn flash_copied(&mut self, target: Copied, cx: &mut Context<Self>) {
        self.copied = Some(target);
        self.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FEEDBACK).await;
            this.update(cx, |companion, cx| {
                companion.copied = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Reads the reply at `index` aloud with macOS `say`, or stops it if it
    /// is already being read.
    fn toggle_speech(&mut self, index: usize, cx: &mut Context<Self>) {
        let already_speaking = self
            .speech
            .as_ref()
            .is_some_and(|speech| speech.index == index);
        self.speech = None;
        if already_speaking {
            cx.notify();
            return;
        }
        let Some(message) = self.state.transcript.get(index) else {
            return;
        };
        let text = self.documents.get(&message.text).plain_text();
        let child = smol::process::Command::new("/usr/bin/say")
            .args(["-f", "-"])
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                eprintln!("claude-companion: couldn't start say: {error}");
                return;
            }
        };
        let task = cx.spawn(async move |this, cx| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes()).await.ok();
            }
            child.status().await.ok();
            this.update(cx, |companion, cx| {
                if companion
                    .speech
                    .as_ref()
                    .is_some_and(|speech| speech.index == index)
                {
                    companion.speech = None;
                    cx.notify();
                }
            })
            .ok();
        });
        self.speech = Some(Speech { index, _task: task });
        cx.notify();
    }

    /// A press on transcript text starts a selection; anywhere else clears
    /// it and may start moving the panel. Text always wins over dragging.
    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hit = match self.view {
            View::Chat => self.text_registry.hit(event.position),
            View::Settings => None,
        };
        match hit {
            Some(point) => {
                self.selection = Some(Selection::at(point));
                self.selecting = true;
                self.drag_origin = None;
            }
            None => {
                self.selection = None;
                self.drag_origin = Some(event.position);
            }
        }
        // In the chat, keystrokes always belong to the input: keep focus
        // there instead of letting the panel's root take it on click.
        if self.view == View::Chat {
            window.focus(&self.input.read(cx).focus_handle(cx));
            window.prevent_default();
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_origin = None;
        if std::mem::take(&mut self.selecting)
            && self.selection.is_some_and(|selection| selection.is_empty())
        {
            // A click, not a drag: leave nothing selected.
            self.selection = None;
            cx.notify();
        }
    }

    /// Extends a selection in progress, or moves the panel once a press
    /// away from text (and outside the text input, which keeps drags for its
    /// own selection) has moved past the threshold.
    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            if event.pressed_button != Some(MouseButton::Left) {
                self.selecting = false;
                return;
            }
            self.pointer = event.position;
            self.extend_selection(cx);
            if self.autoscroll_step() != 0. && self.autoscroll.is_none() {
                self.start_autoscroll(cx);
            }
            return;
        }
        let Some(origin) = self.drag_origin else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) || self.input.read(cx).is_selecting() {
            self.drag_origin = None;
            return;
        }
        let moved = event.position - origin;
        if moved.x.abs() > px(DRAG_THRESHOLD) || moved.y.abs() > px(DRAG_THRESHOLD) {
            self.drag_origin = None;
            if let Some(panel) = self.panel {
                panel.begin_drag();
            }
        }
    }

    /// Moves the selection's head to the text nearest the pointer.
    fn extend_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self.selection.as_mut()
            && let Some(head) = self.text_registry.nearest(self.pointer)
            && selection.head != head
        {
            selection.head = head;
            cx.notify();
        }
    }

    /// How far to scroll the chat this step for the current pointer:
    /// negative scrolls up, 0 when the pointer isn't near an edge. Faster
    /// the deeper the pointer is in the edge zone.
    fn autoscroll_step(&self) -> f32 {
        let bounds = self.scroll.bounds();
        let zone = px(AUTOSCROLL_ZONE);
        let y = self.pointer.y;
        let depth = if y < bounds.top() + zone {
            -f32::from(bounds.top() + zone - y)
        } else if y > bounds.bottom() - zone {
            f32::from(y - (bounds.bottom() - zone))
        } else {
            return 0.;
        };
        (depth / 2.).clamp(-AUTOSCROLL_MAX_STEP, AUTOSCROLL_MAX_STEP)
    }

    /// Scrolls the chat every frame while a selection is held at its edge,
    /// extending the selection with it. Stops when the pointer leaves the
    /// edge or the button is released, even outside the panel, where gpui
    /// sends no mouse-up.
    fn start_autoscroll(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AUTOSCROLL_INTERVAL).await;
                let keep_going = this
                    .update(cx, |companion, cx| {
                        if !companion.selecting || !Panel::left_button_down() {
                            companion.selecting = false;
                            return false;
                        }
                        let step = companion.autoscroll_step();
                        if step == 0. {
                            return false;
                        }
                        let max = companion.scroll.max_offset().height;
                        let mut offset = companion.scroll.offset();
                        offset.y = (offset.y - px(step)).clamp(-max, px(0.));
                        companion.scroll.set_offset(offset);
                        companion.extend_selection(cx);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
            this.update(cx, |companion, _| companion.autoscroll = None)
                .ok();
        }));
    }

    /// No message sent and no reply streaming yet.
    fn chat_started(&self) -> bool {
        !self.state.transcript.is_empty() || self.pending.is_some()
    }

    /// Whether clicking away hides the panel. Before a chat starts it always
    /// does, like a launcher; once one has, the setting decides.
    fn hides_on_blur(&self) -> bool {
        !self.chat_started() || self.settings.hide_on_blur
    }

    fn is_compact(&self) -> bool {
        self.view == View::Chat && !self.chat_started()
    }

    /// The panel's corner radius: the card's in the compact bar, which is
    /// the card on its own, else the panel's own.
    fn corner_radius(&self) -> f32 {
        if self.is_compact() {
            theme::COMPACT_RADIUS
        } else {
            theme::PANEL_RADIUS
        }
    }

    /// Resizes the panel (and sets its corner radius) to suit the current
    /// view.
    fn sync_height(&mut self, cx: &mut Context<Self>) {
        let height = match self.view {
            View::Settings => SETTINGS_HEIGHT,
            View::Chat if self.is_compact() => {
                let mut height = COMPACT_HEIGHT;
                if !self.attachments.is_empty() {
                    height += THUMBNAIL_SIZE + COMPOSER_ROW_GAP;
                }
                if self.attach_notice.is_some() {
                    height += NOTICE_HEIGHT + COMPOSER_ROW_GAP;
                }
                height
            }
            View::Chat => EXPANDED_HEIGHT,
        };
        let radius = self.corner_radius();
        if self.height == Some(height) && self.radius == Some(radius) {
            return;
        }
        let Some(panel) = self.panel else {
            return;
        };
        self.height = Some(height);
        self.radius = Some(radius);
        // AppKit reports the resize to gpui synchronously, and gpui drops the
        // report if it arrives while the app is borrowed, which is true
        // inside any update or `cx.defer`. A spawned task runs on the main
        // queue outside that borrow (as gpui's own `Window::resize` does), and
        // it is queued ahead of the next frame, so the new view and size are
        // drawn together.
        cx.spawn(async move |_, _| {
            panel.set_corner_radius(radius);
            panel.set_height(height);
        })
        .detach();
    }

    fn render_reply_actions(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let copied = self.copied == Some(Copied::Reply(index));
        let speaking = self
            .speech
            .as_ref()
            .is_some_and(|speech| speech.index == index);
        let is_latest = index + 1 == self.state.transcript.len();
        div()
            .flex()
            .gap_1()
            .ml(px(-6.))
            .child(
                IconButton::new(
                    ("copy-reply", index),
                    if copied { Icon::Check } else { Icon::Copy },
                )
                .small()
                .on_click(cx.listener(move |this, _, _, cx| this.copy_reply(index, cx))),
            )
            .child(
                IconButton::new(
                    ("speak-reply", index),
                    if speaking { Icon::Stop } else { Icon::Speaker },
                )
                .small()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_speech(index, cx))),
            )
            .when(is_latest && !self.is_busy(), |this| {
                this.child(
                    IconButton::new(("retry-reply", index), Icon::Retry)
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| this.retry(index, cx))),
                )
            })
    }

    fn render_chat(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.text_registry.clear();
        let code_copy = self.code_copy(cx);
        let mut messages = Vec::with_capacity(self.state.transcript.len());
        for (index, message) in self.state.transcript.iter().enumerate() {
            let element = match message.role {
                Role::Assistant => div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(MessageBubble::markdown(
                        format!("message-{index}"),
                        self.documents.get(&message.text),
                        self.selectable(index),
                        code_copy.clone(),
                    ))
                    .child(self.render_reply_actions(index, cx))
                    .into_any_element(),
                Role::User if !message.images.is_empty() => div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap_2()
                    .child(div().flex().flex_wrap().justify_end().gap_2().children(
                        // The wrapper fixes the size and clips; an `img`
                        // sized directly keeps its own aspect ratio.
                        message.images.iter().map(|path| {
                            div()
                                .size(px(SENT_IMAGE_SIZE))
                                .flex_none()
                                .rounded(px(14.))
                                .overflow_hidden()
                                .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
                        }),
                    ))
                    .when(!message.text.is_empty(), |this| {
                        this.child(MessageBubble::text(
                            Role::User,
                            message.text.clone(),
                            self.selectable(index),
                        ))
                    })
                    .into_any_element(),
                role => MessageBubble::text(role, message.text.clone(), self.selectable(index))
                    .into_any_element(),
            };
            messages.push(element);
        }
        let pending_selectable = self.selectable(self.state.transcript.len());
        let pending = self.pending.as_ref().map(|pending| {
            render_pending(pending, &mut self.documents, pending_selectable, code_copy)
        });
        self.documents.sweep();
        scroll::lock_axis(div().id("transcript").flex_1().overflow_y_scroll())
            .track_scroll(&self.scroll)
            .px_6()
            .pt_2()
            // The space above the composer lives here rather than as the
            // card's top margin, so scrolled messages clip right at the
            // card's edge instead of a margin's width above it.
            .pb(px(12. + theme::COMPOSER_MARGIN))
            .flex()
            .flex_col()
            .gap_5()
            .children(messages)
            .children(pending)
            .into_any_element()
    }

    fn render_thumbnail(&self, index: usize, path: PathBuf, cx: &mut Context<Self>) -> AnyElement {
        let remove = div()
            .id(("remove-attachment", index))
            .absolute()
            .top(px(3.))
            .right(px(3.))
            .size(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(theme::send_ready_icon())
            .on_click(cx.listener(move |this, _, _, cx| this.remove_attachment(index, cx)))
            .child(Icon::Close.render(11., theme::text()));
        div()
            .relative()
            .size(px(THUMBNAIL_SIZE))
            .flex_none()
            .rounded(px(10.))
            .overflow_hidden()
            .border_1()
            .border_color(theme::window_border())
            .child(img(path).size_full().object_fit(ObjectFit::Cover))
            .child(remove)
            .into_any_element()
    }

    fn render_composer(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.can_send(cx);
        let send = IconButton::new("send", Icon::ArrowUp)
            .when(ready, |this| {
                this.background(theme::send_ready())
                    .color(theme::send_ready_icon())
            })
            .when(!ready, |this| {
                this.background(theme::send_idle())
                    .color(theme::muted_text())
            })
            .on_click(cx.listener(|this, _, _, cx| this.submit(cx)));
        let tools = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                IconButton::new("attach", Icon::Plus)
                    .on_click(cx.listener(|this, _, window, cx| this.pick_images(window, cx))),
            )
            .child(
                IconButton::new("open-settings", Icon::Settings)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
            );

        // Compact, the panel is the card (at the card's radius); expanded,
        // the card is inset in it. Either way the buttons sit the card's
        // inset from an edge whose radius is theirs plus that inset.
        let inset = theme::COMPOSER_INSET;
        let mut thumbnails = Vec::with_capacity(self.attachments.len());
        for (index, path) in self.attachments.iter().enumerate() {
            thumbnails.push(self.render_thumbnail(index, path.clone(), cx));
        }
        let thumbnails = (!thumbnails.is_empty()).then(|| {
            div()
                .flex()
                .gap_2()
                .px(px(inset + TEXT_INSET))
                .children(thumbnails)
        });
        let notice = self.attach_notice.clone().map(|notice| {
            div()
                .h(px(NOTICE_HEIGHT))
                .px(px(inset + TEXT_INSET))
                .text_size(px(12.))
                .text_color(theme::error())
                .child(notice)
        });
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(COMPOSER_ROW_GAP))
            .when(compact, |this| this.size_full().justify_between())
            .when(!compact, |this| {
                this.mx(px(theme::COMPOSER_MARGIN))
                    .mb(px(theme::COMPOSER_MARGIN))
                    .rounded(px(theme::COMPOSER_RADIUS))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::window_border())
            })
            .pt(px(inset + TEXT_INSET))
            .children(thumbnails)
            .children(notice)
            .child(
                div()
                    .px(px(inset + TEXT_INSET))
                    .text_size(px(16.))
                    .line_height(px(INPUT_LINE_HEIGHT))
                    .child(self.input.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(inset))
                    .pb(px(inset))
                    .child(tools)
                    .child(send),
            )
    }
}

fn render_pending(
    pending: &Pending,
    documents: &mut DocumentCache,
    selectable: Selectable,
    code_copy: CodeCopy,
) -> AnyElement {
    let status = pending.status.clone().unwrap_or_else(|| "Thinking…".into());
    let status_line = div()
        .text_xs()
        .text_color(theme::muted_text())
        .child(status);
    if pending.reply.is_empty() {
        return status_line.into_any_element();
    }
    let bubble = MessageBubble::markdown(
        "pending",
        documents.get(&pending.reply),
        selectable,
        code_copy,
    );
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(bubble)
        .children(pending.status.is_some().then_some(status_line))
        .into_any_element()
}

fn tool_label(name: &str) -> SharedString {
    match name {
        "web_search" | "WebSearch" => "Searching the web…".into(),
        "web_fetch" | "WebFetch" => "Reading a page…".into(),
        other => format!("Using {other}…").into(),
    }
}

impl Render for Companion {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Normally already done by `observe_self`; this catches the first
        // frame.
        self.sync_height(cx);
        let compact = self.is_compact();
        let content =
            match self.view {
                View::Settings => self.settings_panel.clone().into_any_element(),
                View::Chat if compact => self.render_composer(true, cx).into_any_element(),
                View::Chat => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        // Its own bar, so scrolled text and pinned code headers
                        // stop below the button instead of passing under it.
                        div()
                            .flex_none()
                            .h(px(TOP_BAR_HEIGHT))
                            .flex()
                            .items_center()
                            .pl(px(CLOSE_INSET))
                            .child(IconButton::new("close", Icon::Close).small().on_click(
                                cx.listener(|this, _, window, cx| this.close(window, cx)),
                            )),
                    )
                    .child(self.render_chat(cx))
                    .child(self.render_composer(false, cx))
                    .into_any_element(),
            };

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_hide))
            .on_action(cx.listener(Self::on_new_chat))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::paste_image))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.attach_files(paths.paths(), cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(self.corner_radius()))
            .border_1()
            .border_color(theme::window_border())
            .bg(theme::background())
            .text_color(theme::text())
            .child(content)
    }
}

/// Applies the native styling and moves the panel back to where it was left.
fn set_up_panel(panel: Panel) {
    panel.configure();
    if let Some(placement) = Placement::load() {
        panel.restore(placement);
    }
}
