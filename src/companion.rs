use std::time::Duration;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, KeyBinding, ScrollHandle, SharedString,
    Subscription, Task, Window, actions, div, prelude::*, px,
};

use crate::{
    claude::{self, Event},
    hotkey,
    message_bubble::MessageBubble,
    panel,
    settings::{Choice, Settings},
    settings_panel::{SettingsEvent, SettingsPanel},
    state::{Role, State},
    text_input::{Submit, TextInput},
    theme,
};

const CONTEXT: &str = "Companion";

/// How long focus must stay elsewhere before hide-on-blur hides the panel.
const BLUR_GRACE: Duration = Duration::from_millis(150);

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
    visible: bool,
    _subscriptions: Vec<Subscription>,
}

impl Companion {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Ask Claude…", cx));
        let settings_panel = cx.new(|_| SettingsPanel::new(settings.clone()));
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &Submit, _, cx| this.submit(cx)),
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
        panel::show(window);
        self.focus_view(window, cx);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.blur_check = None;
        panel::hide(window);
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

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.input.read(cx).text().trim().is_empty() {
            return;
        }
        if !self.ensure_session(cx) {
            return;
        }
        let prompt = self.input.update(cx, |input, cx| input.take(cx));
        let prompt = prompt.trim();
        self.state.push(Role::User, prompt);
        self.save();

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

    fn render_chat(&self) -> AnyElement {
        if self.state.transcript.is_empty() && self.pending.is_none() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme::muted_text())
                .child(format!(
                    "Conversations reset after {} idle.",
                    self.settings.session_timeout.label()
                ))
                .into_any_element();
        }
        let messages = self
            .state
            .transcript
            .iter()
            .map(|message| MessageBubble::new(message.role, message.text.clone()));
        div()
            .id("transcript")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .children(messages)
            .children(self.pending.as_ref().map(render_pending))
            .into_any_element()
    }

    fn render_composer(&self) -> impl IntoElement {
        div()
            .flex_none()
            .p_3()
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::surface())
                    .text_sm()
                    .line_height(px(20.))
                    .child(self.input.clone()),
            )
    }
}

fn render_pending(pending: &Pending) -> AnyElement {
    let status = pending.status.clone().unwrap_or_else(|| "Thinking…".into());
    let status_line = div()
        .text_xs()
        .text_color(theme::muted_text())
        .child(status);
    if pending.reply.is_empty() {
        return status_line.into_any_element();
    }
    let bubble = MessageBubble::new(Role::Assistant, pending.reply.clone());
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
        let (hint, button_label) = match self.view {
            View::Chat => ("esc to hide · ⌘N new chat", "Settings"),
            View::Settings => ("esc to go back", "Back"),
        };
        let settings_button = div()
            .id("settings-button")
            .px_2()
            .py(px(2.))
            .rounded_md()
            .cursor_pointer()
            .text_color(theme::text())
            .bg(theme::surface())
            .hover(|style| style.bg(theme::user_bubble()))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx)))
            .child(button_label);
        let header = div()
            .h(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .text_xs()
            .text_color(theme::muted_text())
            .child(hint)
            .child(settings_button);

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_hide))
            .on_action(cx.listener(Self::on_new_chat))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::background())
            .text_color(theme::text())
            .child(header)
            .map(|this| match self.view {
                View::Chat => this.child(self.render_chat()).child(self.render_composer()),
                View::Settings => this.child(self.settings_panel.clone()),
            })
    }
}
