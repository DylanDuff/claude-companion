use std::time::Duration;

use gpui::{
    AnyElement, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollHandle, SharedString, Subscription, Task,
    Window, actions, div, prelude::*, px,
};
use smol::io::AsyncWriteExt;

use crate::{
    claude::{self, Event},
    hotkey,
    icon::{Icon, IconButton},
    markdown::DocumentCache,
    message_bubble::MessageBubble,
    panel::{self, Panel},
    settings::Settings,
    settings_panel::{SettingsEvent, SettingsPanel},
    state::{Role, State},
    text_input::{Submit, TextInput},
    theme,
};

const CONTEXT: &str = "Companion";

/// How long focus must stay elsewhere before hide-on-blur hides the panel.
const BLUR_GRACE: Duration = Duration::from_millis(150);

/// Panel height with no conversation: just the composer.
const COMPACT_HEIGHT: f32 = 96.;
/// Panel height once there is a conversation or the settings are open.
const EXPANDED_HEIGHT: f32 = 540.;

/// How far the mouse must move while pressed before the panel starts moving,
/// so plain clicks still reach buttons.
const DRAG_THRESHOLD: f32 = 4.;

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
    /// Transcript index of the reply whose Copy button shows a tick.
    copied: Option<usize>,
    copied_reset: Option<Task<()>>,
    /// The native panel, configured once at creation.
    panel: Option<Panel>,
    /// The panel height last requested, so it is only resized on change.
    height: Option<f32>,
    /// Where the left button went down, while a window drag may still start.
    drag_origin: Option<Point<Pixels>>,
    visible: bool,
    _subscriptions: Vec<Subscription>,
}

impl Companion {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Ask anything", cx));
        let settings_panel = cx.new(|_| SettingsPanel::new(settings.clone()));
        let panel = Panel::new(window);
        if let Some(panel) = panel {
            panel.configure();
        }
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &Submit, _, cx| this.submit(cx)),
            // Re-render as the text changes, so the send button follows it.
            cx.observe(&input, |_, _, cx| cx.notify()),
            // Every state change notifies; resize then, before the frame is
            // drawn, so the new layout never appears at the old size.
            cx.observe_self(|this, cx| this.sync_height(cx)),
            cx.subscribe_in(
                &settings_panel,
                window,
                |this, _, event, window, cx| match event {
                    SettingsEvent::Changed(settings) => this.apply_settings(settings.clone(), cx),
                    SettingsEvent::Closed => this.set_view(View::Chat, window, cx),
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && this.settings.hide_on_blur {
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
            height: None,
            drag_origin: None,
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
                panel.configure();
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
                if companion.visible && !window.is_window_active() {
                    companion.hide(window, cx);
                }
            })
            .ok();
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
        self.session = None;
        self.pending = None;
        self.unsettled = false;
        self.needs_result_text = false;
        self.state = State::default();
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

    /// Switches web search and page reading together: off if either is on.
    fn toggle_web(&mut self, cx: &mut Context<Self>) {
        let enabled = !(self.settings.web_search || self.settings.web_fetch);
        let settings = Settings {
            web_search: enabled,
            web_fetch: enabled,
            ..self.settings.clone()
        };
        self.settings_panel
            .update(cx, |panel, cx| panel.reset(settings.clone(), None, cx));
        self.apply_settings(settings, cx);
        cx.notify();
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
        self.pending.is_none() && !self.input.read(cx).text().trim().is_empty()
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_send(cx) {
            return;
        }
        let prompt = self.input.update(cx, |input, cx| input.take(cx));
        let prompt = prompt.trim();
        self.state.push(Role::User, prompt);
        self.save();
        self.dispatch(prompt, cx);
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
        transcript.truncate(question + 1);
        self.speech = None;
        self.save();
        self.dispatch(&prompt, cx);
    }

    /// Sends `prompt` to the running session, starting one if needed.
    fn dispatch(&mut self, prompt: &str, cx: &mut Context<Self>) {
        if !self.ensure_session(cx) {
            return;
        }
        let session = self.session.as_mut().expect("session was just ensured");
        if let Err(error) = session.process.send(prompt) {
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

    fn copy_reply(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(message) = self.state.transcript.get(index) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(message.text.clone()));
        self.copied = Some(index);
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

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_origin = Some(event.position);
        // In the chat, keystrokes always belong to the input: keep focus
        // there instead of letting the panel's root take it on click.
        if self.view == View::Chat {
            window.focus(&self.input.read(cx).focus_handle(cx));
            window.prevent_default();
        }
    }

    /// Moves the panel once a press anywhere on it (except the text input,
    /// which keeps drags for selection) has moved past the threshold.
    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    fn is_compact(&self) -> bool {
        self.view == View::Chat && self.state.transcript.is_empty() && self.pending.is_none()
    }

    /// Resizes the panel when switching between compact and expanded.
    fn sync_height(&mut self, cx: &mut Context<Self>) {
        let height = if self.is_compact() {
            COMPACT_HEIGHT
        } else {
            EXPANDED_HEIGHT
        };
        if self.height == Some(height) {
            return;
        }
        let Some(panel) = self.panel else {
            return;
        };
        self.height = Some(height);
        // AppKit reports the resize to gpui synchronously, and gpui drops the
        // report if it arrives while the app is borrowed, which is true
        // inside any update or `cx.defer`. A spawned task runs on the main
        // queue outside that borrow (as gpui's own `Window::resize` does), and
        // it is queued ahead of the next frame, so the new view and size are
        // drawn together.
        cx.spawn(async move |_, _| panel.set_height(height))
            .detach();
    }

    fn render_reply_actions(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let copied = self.copied == Some(index);
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
                    ))
                    .child(self.render_reply_actions(index, cx))
                    .into_any_element(),
                role => MessageBubble::text(role, message.text.clone()).into_any_element(),
            };
            messages.push(element);
        }
        let pending = self
            .pending
            .as_ref()
            .map(|pending| render_pending(pending, &mut self.documents));
        self.documents.sweep();
        div()
            .id("transcript")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px_6()
            .pt_6()
            .pb_3()
            .flex()
            .flex_col()
            .gap_5()
            .children(messages)
            .children(pending)
            .into_any_element()
    }

    fn render_composer(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let web_on = self.settings.web_search || self.settings.web_fetch;
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
                IconButton::new("new-chat", Icon::Plus)
                    .on_click(cx.listener(|this, _, _, cx| this.new_chat(cx))),
            )
            .child(
                IconButton::new("toggle-web", Icon::Globe)
                    .color(if web_on {
                        theme::link().into()
                    } else {
                        theme::muted_text()
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_web(cx))),
            )
            .child(
                IconButton::new("open-settings", Icon::Settings)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
            );

        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .when(compact, |this| this.size_full().justify_between())
            .when(!compact, |this| {
                this.m_3()
                    .rounded(px(20.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::window_border())
            })
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .child(self.input.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .pb_2()
                    .child(tools)
                    .child(send),
            )
    }
}

fn render_pending(pending: &Pending, documents: &mut DocumentCache) -> AnyElement {
    let status = pending.status.clone().unwrap_or_else(|| "Thinking…".into());
    let status_line = div()
        .text_xs()
        .text_color(theme::muted_text())
        .child(status);
    if pending.reply.is_empty() {
        return status_line.into_any_element();
    }
    let bubble = MessageBubble::markdown("pending", documents.get(&pending.reply));
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
        let content = match self.view {
            View::Settings => self.settings_panel.clone().into_any_element(),
            View::Chat if compact => self.render_composer(true, cx).into_any_element(),
            View::Chat => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
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
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag_origin = None),
            )
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(panel::CORNER_RADIUS))
            .border_1()
            .border_color(theme::window_border())
            .bg(theme::background())
            .text_color(theme::text())
            .child(content)
    }
}
