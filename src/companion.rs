use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, KeyBinding, ScrollHandle, SharedString,
    Subscription, Task, Window, actions, div, prelude::*, px,
};

use crate::{
    claude::{self, Event},
    message_bubble::MessageBubble,
    state::{Role, State},
    text_input::{Submit, TextInput},
    theme,
};

const CONTEXT: &str = "Companion";

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
    finished: Option<(Option<String>, Result<String, String>)>,
}

pub struct Companion {
    state: State,
    input: Entity<TextInput>,
    pending: Option<Pending>,
    /// Owns the running `claude` process; dropping it kills the process.
    reply_task: Option<Task<()>>,
    scroll: ScrollHandle,
    visible: bool,
    _subscriptions: Vec<Subscription>,
}

impl Companion {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Ask Claude…", cx));
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &Submit, _, cx| this.submit(cx)),
            // Behave like Spotlight: clicking away hides the window.
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.hide(cx);
                }
            }),
        ];
        Self {
            state: State::load(),
            input,
            pending: None,
            reply_task: None,
            scroll: ScrollHandle::new(),
            visible: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible && window.is_window_active() {
            self.hide(cx);
        } else {
            self.show(window, cx);
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Never expire mid-reply: the reply resets the clock when it lands.
        if self.pending.is_none() && self.state.expire_if_stale() {
            self.save();
        }
        self.visible = true;
        cx.activate(true);
        window.activate_window();
        window.focus(&self.input.focus_handle(cx));
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        cx.hide();
    }

    fn on_hide(&mut self, _: &Hide, _: &mut Window, cx: &mut Context<Self>) {
        self.hide(cx);
    }

    fn on_new_chat(&mut self, _: &NewChat, _: &mut Window, cx: &mut Context<Self>) {
        self.reply_task = None;
        self.pending = None;
        self.state = State::default();
        self.save();
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.input.read(cx).text().trim().is_empty() {
            return;
        }
        let prompt = self.input.update(cx, |input, cx| input.take(cx));
        self.state.push(Role::User, prompt.trim());
        self.save();

        let session_id = self.state.session_id.clone();
        self.pending = Some(Pending::default());
        self.reply_task = Some(cx.spawn(async move |this, cx| {
            let result = claude::send(&prompt, session_id.as_deref(), |event| {
                this.update(cx, |companion, cx| companion.apply(event, cx))
                    .is_ok()
            })
            .await;
            this.update(cx, |companion, cx| companion.finish(result, cx))
                .ok();
        }));
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        match event {
            Event::MessageStarted => {
                if !pending.reply.is_empty() && !pending.reply.ends_with("\n\n") {
                    pending.reply.push_str("\n\n");
                }
            }
            Event::TextDelta(text) => {
                pending.reply.push_str(&text);
                pending.status = None;
            }
            Event::ToolStarted(name) => pending.status = Some(tool_label(&name)),
            Event::Finished {
                session_id,
                outcome,
            } => pending.finished = Some((session_id, outcome)),
        }
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Moves the finished reply into the transcript. `result` is the process
    /// outcome, used only when Claude never sent a result of its own.
    fn finish(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match (pending.finished, result) {
            (Some((session_id, Ok(text))), _) => {
                // The streamed text includes any preamble before a search;
                // the result only holds the final message.
                let reply = if pending.reply.trim().is_empty() {
                    text
                } else {
                    pending.reply
                };
                self.state.push(Role::Assistant, reply.trim());
                if session_id.is_some() {
                    self.state.session_id = session_id;
                }
                self.state.touch();
            }
            (Some((_, Err(reason))), _) | (None, Err(reason)) => {
                self.state.push(Role::Error, reason);
            }
            (None, Ok(())) => self
                .state
                .push(Role::Error, "claude exited without a reply"),
        }
        self.save();
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn save(&self) {
        if let Err(error) = self.state.save() {
            eprintln!("claude-companion: couldn't save state: {error}");
        }
    }

    fn render_transcript(&self) -> impl IntoElement {
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
    }

    fn render_empty(&self) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(theme::muted_text())
            .child("Conversations reset after 5 minutes idle.")
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

impl Focusable for Companion {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for Companion {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_empty = self.state.transcript.is_empty() && self.pending.is_none();
        let header = div()
            .h(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme::muted_text())
            .child("esc to hide · ⌘N new chat");
        let composer = div()
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
            );

        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_hide))
            .on_action(cx.listener(Self::on_new_chat))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::background())
            .text_color(theme::text())
            .child(header)
            .map(|this| {
                if is_empty {
                    this.child(self.render_empty())
                } else {
                    this.child(self.render_transcript())
                }
            })
            .child(composer)
    }
}
