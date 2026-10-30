//! The Redis tools window: pub/sub, command monitor, slow log and ACL users.
//!
//! It is a separate OS window (like the export/import wizards) because the live streams and the
//! command lists must not run on the main window's event loop. Every operation goes through the
//! generic [`rustgrid_core::Connection`] API (`watch` for the streams, `execute_query` for the
//! command-based sections), so the window itself never mentions an engine id.

use rustgrid_core::{WatchEvent, WatchSpec};

use super::*;

/// The sections of the Redis tools window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RedisToolsSection {
    Keys,
    PubSub,
    Monitor,
    Slowlog,
    Acl,
}

impl RedisToolsSection {
    fn label_key(self) -> &'static str {
        match self {
            Self::Keys => "redis.tools.keys",
            Self::PubSub => "redis.tools.pubsub",
            Self::Monitor => "redis.tools.monitor",
            Self::Slowlog => "redis.tools.slowlog",
            Self::Acl => "redis.tools.acl",
        }
    }
}

/// The state of the open Redis tools window.
pub(super) struct RedisToolsState {
    pub(super) connection_index: usize,
    pub(super) database_index: usize,
    pub(super) database: String,
    pub(super) section: RedisToolsSection,
    pub(super) channel_input: Entity<TextInput>,
    pub(super) message_input: Entity<TextInput>,
    pub(super) acl_name_input: Entity<TextInput>,
    pub(super) acl_rules_input: Entity<TextInput>,
    /// The 键 section's fields.
    pub(super) key_name_input: Entity<TextInput>,
    pub(super) key_field_input: Entity<TextInput>,
    pub(super) key_value_input: Entity<TextInput>,
    pub(super) key_ttl_input: Entity<TextInput>,
    pub(super) key_kind: String,
    /// The namespaces of the current database, with their key counts.
    pub(super) namespaces: Vec<rustgrid_core::ObjectNamespace>,
    /// The namespace filter currently applied to the object list.
    pub(super) namespace_pattern: Option<String>,
    /// Subscribed channels and patterns.
    pub(super) channels: Vec<String>,
    /// `(time, channel, text)` for the message log.
    pub(super) messages: Vec<(String, String, String)>,
    /// `MONITOR` lines.
    pub(super) monitor: Vec<String>,
    /// `SLOWLOG GET` entries, pre-rendered.
    pub(super) slowlog: Vec<String>,
    /// `ACL LIST` lines.
    pub(super) acl_users: Vec<String>,
    pub(super) acl_selected: Option<String>,
    /// The running stream's task, aborted when the stream is stopped or the window closes.
    pub(super) stream: Option<tokio::task::JoinHandle<()>>,
    /// Whether the section's stream is running (for the button state).
    pub(super) streaming: bool,
    /// Whether the last operation was for the monitor stream (the events go to that log).
    pub(super) stream_is_monitor: bool,
    /// The last error, shown at the bottom of the window.
    pub(super) error: Option<String>,
}

/// The root view of the Redis tools OS window.
pub(super) struct RedisToolsWindow {
    app: WeakEntity<AppView>,
    _subscription: Subscription,
}

impl RedisToolsWindow {
    pub(super) fn new(
        app: WeakEntity<AppView>,
        app_entity: &Entity<AppView>,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let subscription = cx.observe(app_entity, |_, _, cx| cx.notify());
        Self {
            app,
            _subscription: subscription,
        }
    }
}

impl Render for RedisToolsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let Some(app) = self.app.upgrade() else {
            return div().into_any_element();
        };
        app.update(cx, |app, cx| app.redis_tools_contents(cx))
    }
}

/// One message-log line, capped so a busy channel cannot grow the window's state without bound.
const REDIS_LOG_LIMIT: usize = 2000;

fn now_time() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

impl AppView {
    /// Opens the Redis tools window for one connection/database (or raises the open one).
    ///
    /// `section` selects the initial pane (the object toolbar's 新建键 / 设置 TTL open the 键 pane);
    /// `preset_key` fills the key field for the TTL action.
    pub(super) fn open_redis_tools(
        &mut self,
        connection_index: usize,
        database_index: usize,
        database: String,
        section: RedisToolsSection,
        preset_key: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(handle) = self.redis_tools_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            let database_name = database.clone();
            if let Some(state) = self.redis_tools.as_mut() {
                if state.connection_index != connection_index || state.database != database_name {
                    // The window follows the connection/database it was reopened for; do not
                    // keep streaming the previous server.
                    if let Some(stream) = state.stream.take() {
                        stream.abort();
                    }
                    state.streaming = false;
                    state.connection_index = connection_index;
                    state.database_index = database_index;
                    state.database = database_name;
                    state.namespace_pattern = None;
                    state.messages.clear();
                    state.monitor.clear();
                    state.slowlog.clear();
                    state.acl_users.clear();
                    state.acl_selected = None;
                }
                state.section = section;
                if let Some(key) = preset_key {
                    state
                        .key_name_input
                        .update(cx, |input, cx| input.set_text(key, cx));
                }
            }
            if section == RedisToolsSection::Keys {
                self.redis_tools_refresh_namespaces(cx);
            } else if section == RedisToolsSection::Slowlog {
                self.redis_tools_refresh_slowlog(cx);
            } else if section == RedisToolsSection::Acl {
                self.redis_tools_refresh_acl(cx);
            }
            cx.notify();
            return;
        }
        self.redis_tools_window = None;
        self.redis_tools = None;

        let theme = self.theme;
        let channel = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.channel_placeholder").to_string().into(),
                    clearable: true,
                    ..Default::default()
                },
                cx,
            )
        });
        let message = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.message_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        let acl_name = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.acl_name_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        let acl_rules = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.acl_rules_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        let key_name = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.key_name_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        if let Some(key) = preset_key.as_ref() {
            let key = key.clone();
            key_name.update(cx, |input, cx| input.set_text(key, cx));
        }
        let key_field = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.key_field_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        let key_value = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.key_value_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        let key_ttl = cx.new(|cx| {
            TextInput::new(
                theme,
                "",
                TextInputOptions {
                    placeholder: t!("redis.tools.key_ttl_placeholder").to_string().into(),
                    ..Default::default()
                },
                cx,
            )
        });
        let namespace_pattern = self
            .connections
            .get(connection_index)
            .and_then(|node| match &node.databases {
                Loadable::Loaded(databases) => databases.get(database_index),
                _ => None,
            })
            .and_then(|database| database.object_pattern.clone());
        self.redis_tools = Some(RedisToolsState {
            connection_index,
            database_index,
            database,
            section,
            channel_input: channel,
            message_input: message,
            acl_name_input: acl_name,
            acl_rules_input: acl_rules,
            key_name_input: key_name,
            key_field_input: key_field,
            key_value_input: key_value,
            key_ttl_input: key_ttl,
            key_kind: "string".to_string(),
            namespaces: Vec::new(),
            namespace_pattern,
            channels: Vec::new(),
            messages: Vec::new(),
            monitor: Vec::new(),
            slowlog: Vec::new(),
            acl_users: Vec::new(),
            acl_selected: None,
            stream: None,
            streaming: false,
            stream_is_monitor: false,
            error: None,
        });
        if section == RedisToolsSection::Keys {
            self.redis_tools_refresh_namespaces(cx);
        } else if section == RedisToolsSection::Slowlog {
            self.redis_tools_refresh_slowlog(cx);
        } else if section == RedisToolsSection::Acl {
            self.redis_tools_refresh_acl(cx);
        }

        let weak = cx.weak_entity();
        let app_entity = cx.entity();
        let title = t!("redis.tools.title").to_string();
        let focus = self.redis_tools_focus.clone();
        cx.defer(move |cx: &mut App| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let bounds = Bounds::centered(None, size(px(760.0), px(560.0)), cx);
            let view_weak = weak.clone();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..ui::window_options(title)
                },
                move |window, cx| {
                    #[cfg(target_os = "windows")]
                    crate::win_resize::install(window);
                    window.activate_window();
                    let view =
                        cx.new(|cx| RedisToolsWindow::new(view_weak.clone(), &app_entity, cx));
                    let root = cx.new(|cx| gpui_kit::component::Root::new(view, window, cx));
                    window.focus(&focus, cx);
                    root
                },
            );
            match opened {
                Ok(handle) => app.update(cx, |app, cx| {
                    app.redis_tools_window = Some(handle);
                    cx.notify();
                }),
                Err(error) => app.update(cx, |app, cx| {
                    app.redis_tools = None;
                    app.error_dialog = Some(error.to_string());
                    cx.notify();
                }),
            }
        });
    }

    /// Closes the Redis tools window and stops its stream.
    pub(super) fn redis_tools_close(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(state) = self.redis_tools.as_mut()
            && let Some(stream) = state.stream.take()
        {
            stream.abort();
        }
        self.redis_watch_generation = self.redis_watch_generation.wrapping_add(1);
        self.redis_tools = None;
        self.redis_tools_window = None;
        window.remove_window();
        cx.notify();
    }

    /// The window body: a section list, the active section and an error line.
    pub(super) fn redis_tools_contents(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        if self.redis_tools.is_none() {
            return div().into_any_element();
        }
        let section = self
            .redis_tools
            .as_ref()
            .map(|state| state.section)
            .unwrap_or(RedisToolsSection::PubSub);

        let mut nav = div().flex().flex_col().gap_1().w(px(130.0)).p_2();
        for (index, candidate) in [
            RedisToolsSection::Keys,
            RedisToolsSection::PubSub,
            RedisToolsSection::Monitor,
            RedisToolsSection::Slowlog,
            RedisToolsSection::Acl,
        ]
        .into_iter()
        .enumerate()
        {
            let active = candidate == section;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("redis-tools-nav-{index}")))
                    .flex()
                    .items_center()
                    .h(px(28.0))
                    .px_2()
                    .rounded_sm()
                    .text_size(px(12.5))
                    .cursor_pointer()
                    .when(active, |element| {
                        element
                            .bg(rgb(theme.tree_selected_bg))
                            .text_color(rgb(theme.tree_selected_text))
                    })
                    .when(!active, |element| {
                        element
                            .text_color(rgb(theme.text))
                            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.redis_tools_select(candidate, cx);
                    }))
                    .child(t!(candidate.label_key()).to_string()),
            );
        }

        let body = match section {
            RedisToolsSection::Keys => self.redis_tools_keys(cx),
            RedisToolsSection::PubSub => self.redis_tools_pubsub(cx),
            RedisToolsSection::Monitor => self.redis_tools_monitor(cx),
            RedisToolsSection::Slowlog => self.redis_tools_slowlog(cx),
            RedisToolsSection::Acl => self.redis_tools_acl(cx),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme.dialog_face))
            .text_color(rgb(theme.text))
            .track_focus(&self.redis_tools_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.redis_tools_close(window, cx);
                }
            }))
            .child(ui::window_title_bar(
                t!("redis.tools.title").to_string(),
                theme,
                None,
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(nav.border_r_1().border_color(rgb(theme.border)))
                    .child(div().flex().flex_col().flex_1().min_w(px(0.0)).child(body)),
            )
            .child(self.redis_tools_status(cx))
            .into_any_element()
    }

    fn redis_tools_status(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let error = self
            .redis_tools
            .as_ref()
            .and_then(|state| state.error.clone());
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(28.0))
            .px_3()
            .border_t_1()
            .border_color(rgb(theme.border))
            .text_size(px(11.5))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(if error.is_some() {
                        rgb(theme.danger)
                    } else {
                        rgb(theme.text_muted)
                    })
                    .child(
                        error
                            .clone()
                            .unwrap_or_else(|| t!("redis.tools.ready").to_string()),
                    ),
            )
            .child(ui::dialog_button(
                "redis-tools-close",
                t!("form.close").to_string(),
                false,
                theme,
                cx.listener(|this, _event, window, cx| this.redis_tools_close(window, cx)),
            ))
            .into_any_element()
    }

    fn redis_tools_select(&mut self, section: RedisToolsSection, cx: &mut Context<'_, Self>) {
        if let Some(state) = self.redis_tools.as_mut() {
            if state.section == section {
                return;
            }
            state.section = section;
            state.error = None;
        }
        // Load the command-based sections lazily.
        match section {
            RedisToolsSection::Keys => self.redis_tools_refresh_namespaces(cx),
            RedisToolsSection::Slowlog => self.redis_tools_refresh_slowlog(cx),
            RedisToolsSection::Acl => self.redis_tools_refresh_acl(cx),
            _ => {}
        }
        cx.notify();
    }

    /// The connection behind the tools window, when it is still open.
    fn redis_tools_connection(&self) -> Option<(Arc<dyn Connection>, String)> {
        let state = self.redis_tools.as_ref()?;
        let node = self.connections.get(state.connection_index)?;
        let ConnectionStatus::Connected(connection) = &node.status else {
            return None;
        };
        Some((connection.clone(), state.database.clone()))
    }

    fn redis_tools_set_error(&mut self, message: impl Into<String>, cx: &mut Context<'_, Self>) {
        if let Some(state) = self.redis_tools.as_mut() {
            state.error = Some(message.into());
        }
        cx.notify();
    }

    // ----- keys ----------------------------------------------------------------------------

    fn redis_tools_keys(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(state) = self.redis_tools.as_ref() else {
            return div().into_any_element();
        };
        let mut namespaces = div().flex().flex_row().flex_wrap().gap_1();
        namespaces = namespaces.child(
            div()
                .id("redis-namespace-all")
                .h(px(22.0))
                .px_2()
                .flex()
                .items_center()
                .rounded_sm()
                .text_size(px(11.0))
                .cursor_pointer()
                .when(state.namespace_pattern.is_none(), |element| {
                    element
                        .bg(rgb(theme.brand))
                        .text_color(rgb(theme.dialog_face))
                })
                .when(state.namespace_pattern.is_some(), |element| {
                    element
                        .border_1()
                        .border_color(rgb(theme.border))
                        .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                })
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.redis_tools_pick_namespace(None, cx);
                }))
                .child(t!("redis.tools.namespace_all").to_string()),
        );
        for (index, namespace) in state.namespaces.iter().enumerate() {
            if namespace.name.is_empty() {
                continue;
            }
            let active = state.namespace_pattern.as_deref() == Some(namespace.name.as_str());
            let name = namespace.name.clone();
            let picked = namespace.name.clone();
            namespaces = namespaces.child(
                div()
                    .id(SharedString::from(format!("redis-namespace-{index}")))
                    .h(px(22.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_sm()
                    .text_size(px(11.0))
                    .cursor_pointer()
                    .when(active, |element| {
                        element
                            .bg(rgb(theme.brand))
                            .text_color(rgb(theme.dialog_face))
                    })
                    .when(!active, |element| {
                        element
                            .border_1()
                            .border_color(rgb(theme.border))
                            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.redis_tools_pick_namespace(Some(picked.clone()), cx);
                    }))
                    .child(name)
                    .child(
                        div()
                            .text_size(px(10.0))
                            .child(format!("({})", namespace.objects)),
                    ),
            );
        }
        let mut kinds = div().flex().flex_row().gap_1();
        for kind in ["string", "hash", "list", "set", "zset", "stream"] {
            let active = state.key_kind == kind;
            kinds = kinds.child(
                div()
                    .id(SharedString::from(format!("redis-key-kind-{kind}")))
                    .h(px(24.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .when(active, |element| {
                        element
                            .bg(rgb(theme.brand))
                            .text_color(rgb(theme.dialog_face))
                    })
                    .when(!active, |element| {
                        element
                            .border_1()
                            .border_color(rgb(theme.border))
                            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        if let Some(state) = this.redis_tools.as_mut() {
                            state.key_kind = kind.to_string();
                        }
                        cx.notify();
                    }))
                    .child(kind),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_3()
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.namespaces").to_string()),
                    )
                    .child(namespaces),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.key_name").to_string()),
                    )
                    .child(
                        div()
                            .h(px(24.0))
                            .w(px(320.0))
                            .child(state.key_name_input.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.key_kind").to_string()),
                    )
                    .child(kinds),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(theme.text_muted))
                                    .child(t!("redis.tools.key_field").to_string()),
                            )
                            .child(
                                div()
                                    .h(px(24.0))
                                    .w(px(220.0))
                                    .child(state.key_field_input.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(theme.text_muted))
                                    .child(t!("redis.tools.key_value").to_string()),
                            )
                            .child(
                                div()
                                    .h(px(24.0))
                                    .w(px(260.0))
                                    .child(state.key_value_input.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(theme.text_muted))
                                    .child(t!("redis.tools.key_ttl").to_string()),
                            )
                            .child(
                                div()
                                    .h(px(24.0))
                                    .w(px(110.0))
                                    .child(state.key_ttl_input.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(ui::dialog_button(
                        "redis-key-create",
                        t!("redis.tools.key_create").to_string(),
                        true,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_create_key(cx)),
                    ))
                    .child(ui::dialog_button(
                        "redis-key-ttl",
                        t!("redis.tools.key_apply_ttl").to_string(),
                        false,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_apply_ttl(cx)),
                    ))
                    .child(ui::dialog_button(
                        "redis-key-persist",
                        t!("redis.tools.key_persist").to_string(),
                        false,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_persist_key(cx)),
                    )),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(theme.text_muted))
                    .child(t!("redis.tools.key_hint").to_string()),
            )
            .into_any_element()
    }

    /// Reloads the namespace chips (the 键 section) from the server.
    fn redis_tools_refresh_namespaces(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            return;
        };
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { connection.list_namespaces(&database).await })
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.redis_watch_generation != generation {
                    return;
                }
                if let Ok(Ok(namespaces)) = result
                    && let Some(state) = app.redis_tools.as_mut()
                {
                    state.namespaces = namespaces;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Applies a namespace filter to the object list behind the tools window.
    fn redis_tools_pick_namespace(
        &mut self,
        namespace: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some((connection_index, database_index)) = self
            .redis_tools
            .as_ref()
            .map(|state| (state.connection_index, state.database_index))
        else {
            return;
        };
        if let Some(state) = self.redis_tools.as_mut() {
            state.namespace_pattern = namespace.clone();
        }
        self.set_object_namespace(connection_index, database_index, namespace, cx);
        cx.notify();
    }

    fn redis_tools_create_key(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let name = state
            .key_name_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.is_empty() {
            self.redis_tools_set_error(t!("redis.tools.key_name_required").to_string(), cx);
            return;
        }
        let kind = state.key_kind.clone();
        let field = state
            .key_field_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default();
        let value = state
            .key_value_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default();
        let ttl = state
            .key_ttl_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .parse::<i64>()
            .ok();
        let values = match kind.as_str() {
            "hash" | "stream" => vec![
                ("field".to_string(), Some(field)),
                ("value".to_string(), Some(value)),
            ],
            "zset" => vec![
                ("member".to_string(), Some(field)),
                ("score".to_string(), Some(value)),
            ],
            "list" | "set" => vec![("value".to_string(), Some(value))],
            _ => vec![("value".to_string(), Some(value))],
        };
        let spec = rustgrid_core::ObjectCreate {
            kind: kind.clone(),
            ttl,
            values,
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let connection_index = state.connection_index;
        let database_index = state.database_index;
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        let key = name.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { connection.create_object(&database, &key, &spec).await })
                .await;
            let _ = this.update(cx, |app, cx| match result {
                Ok(Ok(())) => {
                    if app.redis_watch_generation != generation {
                        return;
                    }
                    if let Some(state) = app.redis_tools.as_mut() {
                        state.error = None;
                    }
                    app.redis_tools_reload_connection(connection_index, database_index, cx);
                }
                Ok(Err(error)) => app.redis_tools_set_error(error.to_string(), cx),
                Err(error) => app.redis_tools_set_error(error.to_string(), cx),
            });
        })
        .detach();
    }

    fn redis_tools_apply_ttl(&mut self, cx: &mut Context<'_, Self>) {
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let ttl = state
            .key_ttl_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .parse::<i64>()
            .ok();
        self.redis_tools_set_ttl(ttl, cx);
    }

    fn redis_tools_persist_key(&mut self, cx: &mut Context<'_, Self>) {
        self.redis_tools_set_ttl(None, cx);
    }

    fn redis_tools_set_ttl(&mut self, ttl: Option<i64>, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let key = state
            .key_name_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        if key.is_empty() {
            self.redis_tools_set_error(t!("redis.tools.key_name_required").to_string(), cx);
            return;
        }
        let connection_index = state.connection_index;
        let database_index = state.database_index;
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { connection.set_object_ttl(&database, &key, ttl).await })
                .await;
            let _ = this.update(cx, |app, cx| match result {
                Ok(Ok(())) => {
                    if app.redis_watch_generation != generation {
                        return;
                    }
                    if let Some(state) = app.redis_tools.as_mut() {
                        state.error = None;
                    }
                    app.redis_tools_reload_connection(connection_index, database_index, cx);
                }
                Ok(Err(error)) => app.redis_tools_set_error(error.to_string(), cx),
                Err(error) => app.redis_tools_set_error(error.to_string(), cx),
            });
        })
        .detach();
    }

    /// Reloads the object list behind the tools window after a key change.
    fn redis_tools_reload_connection(
        &mut self,
        connection_index: usize,
        database_index: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(node) = self.connections.get(connection_index) else {
            return;
        };
        let ConnectionStatus::Connected(connection) = &node.status else {
            return;
        };
        let connection = connection.clone();
        let Some(database) = self.database_name(connection_index, database_index) else {
            return;
        };
        self.load_tables(connection_index, database_index, connection, database, cx);
        self.redis_tools_refresh_namespaces(cx);
        cx.notify();
    }

    // ----- pub/sub -------------------------------------------------------------------------

    fn redis_tools_pubsub(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(state) = self.redis_tools.as_ref() else {
            return div().into_any_element();
        };
        let streaming = state.streaming && !state.stream_is_monitor;
        let mut log = div()
            .id("redis-pubsub-log")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p_2()
            .gap_0p5()
            .text_size(px(12.0));
        for (time, channel, text) in &state.messages {
            log = log.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(
                        div()
                            .w(px(64.0))
                            .flex_none()
                            .text_color(rgb(theme.text_muted))
                            .child(time.clone()),
                    )
                    .child(
                        div()
                            .w(px(140.0))
                            .flex_none()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(channel.clone()),
                    )
                    .child(div().flex_1().min_w(px(0.0)).child(text.clone())),
            );
        }
        let channels = state.channels.join(", ");

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(180.0))
                            .h(px(24.0))
                            .child(state.channel_input.clone()),
                    )
                    .child(ui::dialog_button(
                        "redis-pubsub-subscribe",
                        t!("redis.tools.subscribe").to_string(),
                        true,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_subscribe(cx)),
                    ))
                    .child(ui::dialog_button(
                        "redis-pubsub-stop",
                        t!("redis.tools.stop").to_string(),
                        false,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_stop_stream(cx)),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(if channels.is_empty() {
                                t!("redis.tools.no_channels").to_string()
                            } else {
                                channels
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(240.0))
                            .h(px(24.0))
                            .child(state.message_input.clone()),
                    )
                    .child(ui::dialog_button(
                        "redis-pubsub-publish",
                        t!("redis.tools.publish").to_string(),
                        false,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_publish(cx)),
                    ))
                    .child(if streaming {
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.brand))
                            .child(t!("redis.tools.subscribed").to_string())
                    } else {
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.not_subscribed").to_string())
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .border_1()
                    .border_color(rgb(theme.border))
                    .rounded_sm()
                    .bg(rgb(theme.input_bg))
                    .child(log),
            )
            .into_any_element()
    }

    fn redis_tools_subscribe(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let channel = state
            .channel_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut channels = Vec::new();
        let mut patterns = Vec::new();
        for entry in channel.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if entry.contains('*') || entry.contains('?') || entry.contains('[') {
                patterns.push(entry.to_string());
            } else {
                channels.push(entry.to_string());
            }
        }
        if channels.is_empty() && patterns.is_empty() {
            self.redis_tools_set_error(t!("redis.tools.channel_required").to_string(), cx);
            return;
        }
        if let Some(state) = self.redis_tools.as_mut() {
            for channel in &channels {
                if !state.channels.contains(channel) {
                    state.channels.push(channel.clone());
                }
            }
            for pattern in &patterns {
                if !state.channels.contains(pattern) {
                    state.channels.push(pattern.clone());
                }
            }
            state.error = None;
        }
        self.redis_tools_start_stream(
            connection,
            database,
            WatchSpec::Subscribe { channels, patterns },
            false,
            cx,
        );
    }

    fn redis_tools_publish(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let channel = state
            .channel_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        let message = state
            .message_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default();
        if channel.is_empty() {
            self.redis_tools_set_error(t!("redis.tools.channel_required").to_string(), cx);
            return;
        }
        let statement = format!(
            "PUBLISH {channel} \"{}\"",
            message.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { connection.execute_query(Some(&database), &statement).await })
                .await;
            let _ = this.update(cx, |app, cx| match result {
                Ok(Ok(_)) => {
                    if app.redis_watch_generation != generation {
                        return;
                    }
                    if let Some(state) = app.redis_tools.as_mut() {
                        state
                            .messages
                            .push((now_time(), channel.clone(), format!("→ {message}")));
                        if state.messages.len() > REDIS_LOG_LIMIT {
                            state.messages.remove(0);
                        }
                        state.error = None;
                    }
                    cx.notify();
                }
                Ok(Err(error)) => app.redis_tools_set_error(error.to_string(), cx),
                Err(error) => app.redis_tools_set_error(error.to_string(), cx),
            });
        })
        .detach();
    }

    // ----- monitor -------------------------------------------------------------------------

    fn redis_tools_monitor(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(state) = self.redis_tools.as_ref() else {
            return div().into_any_element();
        };
        let mut log = div()
            .id("redis-monitor-log")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p_2()
            .gap_0p5()
            .text_size(px(11.5));
        for line in &state.monitor {
            log = log.child(div().child(line.clone()));
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(ui::dialog_button(
                        "redis-monitor-start",
                        t!("redis.tools.start_monitor").to_string(),
                        true,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_start_monitor(cx)),
                    ))
                    .child(ui::dialog_button(
                        "redis-monitor-stop",
                        t!("redis.tools.stop").to_string(),
                        false,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_stop_stream(cx)),
                    ))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.monitor_hint").to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .border_1()
                    .border_color(rgb(theme.border))
                    .rounded_sm()
                    .bg(rgb(theme.input_bg))
                    .child(log),
            )
            .into_any_element()
    }

    fn redis_tools_start_monitor(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        self.redis_tools_start_stream(connection, database, WatchSpec::Monitor, true, cx);
    }

    // ----- slow log ------------------------------------------------------------------------

    fn redis_tools_slowlog(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(state) = self.redis_tools.as_ref() else {
            return div().into_any_element();
        };
        let mut log = div()
            .id("redis-slowlog-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p_2()
            .gap_0p5()
            .text_size(px(11.5));
        for line in &state.slowlog {
            log = log.child(div().child(line.clone()));
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(ui::dialog_button(
                        "redis-slowlog-refresh",
                        t!("redis.tools.refresh").to_string(),
                        true,
                        theme,
                        cx.listener(|this, _event, _window, cx| {
                            this.redis_tools_refresh_slowlog(cx)
                        }),
                    ))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.slowlog_hint").to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .border_1()
                    .border_color(rgb(theme.border))
                    .rounded_sm()
                    .bg(rgb(theme.input_bg))
                    .child(log),
            )
            .into_any_element()
    }

    fn redis_tools_refresh_slowlog(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move {
                    connection
                        .execute_query(Some(&database), "SLOWLOG GET 100")
                        .await
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.redis_watch_generation != generation {
                    return;
                }
                match result {
                    Ok(Ok(result)) => {
                        let lines: Vec<String> = result
                            .rows
                            .iter()
                            .map(|row| {
                                row.iter()
                                    .map(|cell| cell.as_display())
                                    .collect::<Vec<_>>()
                                    .join("  ")
                            })
                            .collect();
                        if let Some(state) = app.redis_tools.as_mut() {
                            state.slowlog = if lines.is_empty() {
                                vec![t!("redis.tools.empty").to_string()]
                            } else {
                                lines
                            };
                            state.error = None;
                        }
                        cx.notify();
                    }
                    Ok(Err(error)) => app.redis_tools_set_error(error.to_string(), cx),
                    Err(error) => app.redis_tools_set_error(error.to_string(), cx),
                }
            });
        })
        .detach();
    }

    // ----- ACL users -----------------------------------------------------------------------

    fn redis_tools_acl(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(state) = self.redis_tools.as_ref() else {
            return div().into_any_element();
        };
        let mut users = div()
            .id("redis-acl-users")
            .flex()
            .flex_col()
            .w(px(240.0))
            .h_full()
            .overflow_y_scroll()
            .p_1()
            .gap_0p5()
            .text_size(px(11.5));
        for (index, line) in state.acl_users.iter().enumerate() {
            let selected = state.acl_selected.as_deref() == Some(line.as_str());
            let name = acl_user_name(line);
            users = users.child(
                div()
                    .id(SharedString::from(format!("redis-acl-user-{index}")))
                    .h(px(24.0))
                    .px_1()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(selected, |element| {
                        element
                            .bg(rgb(theme.tree_selected_bg))
                            .text_color(rgb(theme.tree_selected_text))
                    })
                    .when(!selected, |element| {
                        element.hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.redis_tools_select_acl(index, cx);
                    }))
                    .child(name),
            );
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .p_3()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(ui::dialog_button(
                        "redis-acl-refresh",
                        t!("redis.tools.refresh").to_string(),
                        false,
                        theme,
                        cx.listener(|this, _event, _window, cx| this.redis_tools_refresh_acl(cx)),
                    ))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(theme.text_muted))
                            .child(t!("redis.tools.acl_hint").to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .border_1()
                            .border_color(rgb(theme.border))
                            .rounded_sm()
                            .bg(rgb(theme.input_bg))
                            .child(users),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(theme.text_muted))
                                            .child(t!("redis.tools.acl_name").to_string()),
                                    )
                                    .child(
                                        div()
                                            .h(px(24.0))
                                            .w(px(240.0))
                                            .child(state.acl_name_input.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(theme.text_muted))
                                            .child(t!("redis.tools.acl_rules").to_string()),
                                    )
                                    .child(div().h(px(24.0)).child(state.acl_rules_input.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(ui::dialog_button(
                                        "redis-acl-save",
                                        t!("redis.tools.acl_save").to_string(),
                                        true,
                                        theme,
                                        cx.listener(|this, _event, _window, cx| {
                                            this.redis_tools_save_acl(cx)
                                        }),
                                    ))
                                    .child(ui::dialog_button(
                                        "redis-acl-delete",
                                        t!("redis.tools.acl_delete").to_string(),
                                        false,
                                        theme,
                                        cx.listener(|this, _event, _window, cx| {
                                            this.redis_tools_delete_acl(cx)
                                        }),
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(theme.text_muted))
                                    .child(t!("redis.tools.acl_rules_hint").to_string()),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn redis_tools_refresh_acl(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { connection.execute_query(Some(&database), "ACL LIST").await })
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.redis_watch_generation != generation {
                    return;
                }
                match result {
                    Ok(Ok(result)) => {
                        let lines: Vec<String> = result
                            .rows
                            .iter()
                            .filter_map(|row| row.first())
                            .map(|cell| cell.as_display())
                            .collect();
                        if let Some(state) = app.redis_tools.as_mut() {
                            state.acl_users = if lines.is_empty() {
                                vec![t!("redis.tools.empty").to_string()]
                            } else {
                                lines
                            };
                            state.error = None;
                        }
                        cx.notify();
                    }
                    Ok(Err(error)) => app.redis_tools_set_error(error.to_string(), cx),
                    Err(error) => app.redis_tools_set_error(error.to_string(), cx),
                }
            });
        })
        .detach();
    }

    fn redis_tools_select_acl(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let Some(line) = state.acl_users.get(index).cloned() else {
            return;
        };
        let (name, rules) = split_acl_line(&line);
        if let Some(state) = self.redis_tools.as_mut() {
            state.acl_selected = Some(line);
            state.acl_name_input.update(cx, |input, cx| {
                input.set_text(name, cx);
            });
            state.acl_rules_input.update(cx, |input, cx| {
                input.set_text(rules, cx);
            });
        }
        cx.notify();
    }

    fn redis_tools_save_acl(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let name = state
            .acl_name_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        let rules = state
            .acl_rules_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.is_empty() {
            self.redis_tools_set_error(t!("redis.tools.acl_name_required").to_string(), cx);
            return;
        }
        let statement = if rules.is_empty() {
            format!("ACL SETUSER {name}")
        } else {
            format!("ACL SETUSER {name} {rules}")
        };
        self.redis_tools_run_admin(connection, database, statement, cx);
    }

    fn redis_tools_delete_acl(&mut self, cx: &mut Context<'_, Self>) {
        let Some((connection, database)) = self.redis_tools_connection() else {
            self.redis_tools_set_error(t!("redis.tools.not_connected").to_string(), cx);
            return;
        };
        let Some(state) = self.redis_tools.as_ref() else {
            return;
        };
        let mut name = state
            .acl_name_input
            .read(cx)
            .current_text(cx)
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.is_empty()
            && let Some(selected) = &state.acl_selected
        {
            name = split_acl_line(selected).0;
        }
        if name.is_empty() {
            self.redis_tools_set_error(t!("redis.tools.acl_name_required").to_string(), cx);
            return;
        }
        self.redis_tools_run_admin(connection, database, format!("ACL DELUSER {name}"), cx);
    }

    /// Runs one admin command and reloads the ACL list afterwards.
    fn redis_tools_run_admin(
        &mut self,
        connection: Arc<dyn Connection>,
        database: String,
        statement: String,
        cx: &mut Context<'_, Self>,
    ) {
        let runtime = self.runtime.clone();
        let generation = self.redis_watch_generation;
        cx.spawn(async move |this, cx| {
            let result = runtime
                .spawn(async move { connection.execute_query(Some(&database), &statement).await })
                .await;
            let _ = this.update(cx, |app, cx| match result {
                Ok(Ok(_)) => {
                    if app.redis_watch_generation != generation {
                        return;
                    }
                    if let Some(state) = app.redis_tools.as_mut() {
                        state.error = None;
                    }
                    app.redis_tools_refresh_acl(cx);
                }
                Ok(Err(error)) => app.redis_tools_set_error(error.to_string(), cx),
                Err(error) => app.redis_tools_set_error(error.to_string(), cx),
            });
        })
        .detach();
    }

    // ----- the shared live-stream plumbing --------------------------------------------------

    /// Starts one live stream, replacing any running one.
    fn redis_tools_start_stream(
        &mut self,
        connection: Arc<dyn Connection>,
        database: String,
        spec: WatchSpec,
        monitor: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(state) = self.redis_tools.as_mut()
            && let Some(stream) = state.stream.take()
        {
            stream.abort();
        }
        self.redis_watch_generation = self.redis_watch_generation.wrapping_add(1);
        let generation = self.redis_watch_generation;
        if let Some(state) = self.redis_tools.as_mut() {
            state.streaming = true;
            state.stream_is_monitor = monitor;
            state.error = None;
        }

        let runtime = self.runtime.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<WatchEvent, String>>();
        let handle = runtime.spawn(async move {
            let watch = connection.watch(Some(&database), &spec).await;
            let mut watch = match watch {
                Ok(Some(watch)) => watch,
                Ok(None) => {
                    let _ = tx.send(Err("this connection has no live streams".to_string()));
                    return;
                }
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    return;
                }
            };
            loop {
                match watch.next().await {
                    Ok(Some(event)) => {
                        if tx.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = tx.send(Err(error.to_string()));
                        break;
                    }
                }
            }
        });

        if let Some(state) = self.redis_tools.as_mut() {
            state.stream = Some(handle);
        }
        cx.spawn(async move |this, cx| {
            while let Some(message) = rx.recv().await {
                let _ = this.update(cx, |app, cx| {
                    app.redis_tools_push_event(generation, monitor, message, cx)
                });
            }
        })
        .detach();
    }

    fn redis_tools_stop_stream(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(state) = self.redis_tools.as_mut() {
            if let Some(stream) = state.stream.take() {
                stream.abort();
            }
            state.streaming = false;
        }
        self.redis_watch_generation = self.redis_watch_generation.wrapping_add(1);
        cx.notify();
    }

    fn redis_tools_push_event(
        &mut self,
        generation: u64,
        monitor: bool,
        event: Result<WatchEvent, String>,
        cx: &mut Context<'_, Self>,
    ) {
        if generation != self.redis_watch_generation {
            return;
        }
        let Some(state) = self.redis_tools.as_mut() else {
            return;
        };
        match event {
            Ok(event) => {
                if monitor {
                    state
                        .monitor
                        .push(format!("{}  {}", now_time(), event.text));
                    if state.monitor.len() > REDIS_LOG_LIMIT {
                        state.monitor.remove(0);
                    }
                } else {
                    state.messages.push((now_time(), event.channel, event.text));
                    if state.messages.len() > REDIS_LOG_LIMIT {
                        state.messages.remove(0);
                    }
                }
                state.error = None;
            }
            Err(error) => {
                state.error = Some(error);
                state.streaming = false;
            }
        }
        cx.notify();
    }
}

/// Splits one `ACL LIST` line (`user alice on ~key:* +get`) into the name and the rule text.
fn split_acl_line(line: &str) -> (String, String) {
    let mut parts = line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("user"), Some(name)) => (name.to_string(), parts.collect::<Vec<_>>().join(" ")),
        _ => (line.to_string(), String::new()),
    }
}

/// The user name of one `ACL LIST` line, for the list.
fn acl_user_name(line: &str) -> String {
    split_acl_line(line).0
}
