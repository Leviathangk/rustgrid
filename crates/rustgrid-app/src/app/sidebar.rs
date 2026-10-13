use super::*;

/// The connection status shown by a tree row, reduced to what rendering needs. A failed connection
/// keeps only its state: the error itself is surfaced in a dialog when it happens.
enum TreeStatus {
    Connected,
    Connecting,
    Failed,
    Disconnected,
}

/// A lightweight, owned snapshot of one connection for rendering the tree. `AppView` keeps the
/// real `ConnectionNode`; the snapshot only clones names and flags, never passwords.
struct TreeConnection {
    index: usize,
    /// The connection profile's id, used to place the row in the persisted tree layout.
    id: String,
    name: String,
    /// The engine's icon asset path and how it is drawn, from the driver's descriptor.
    icon: &'static str,
    icon_style: DriverIconStyle,
    /// Whether the engine has stored routines; when false the Functions category is hidden.
    supports_routines: bool,
    /// Whether the engine has schemas; when false the flat category tree is used and the
    /// schema create/drop actions are not offered.
    supports_schemas: bool,
    status: TreeStatus,
    /// The error from the last failed connect, shown as a tooltip on the node.
    failed_error: Option<String>,
    expanded: bool,
    /// The connection's environment colour, tinting this connection's whole subtree (参考 Navicat).
    color: Option<ConnectionColor>,
    databases: Loadable<Vec<TreeDatabase>>,
    /// Saved queries belonging to this connection, across all its databases.
    saved_queries: Vec<TreeSavedQuery>,
}

struct TreeDatabase {
    index: usize,
    name: String,
    opened: bool,
    expanded: bool,
    categories: CategoryExpansion,
    tables: Loadable<Vec<TreeTable>>,
    routines: Loadable<Vec<RoutineInfo>>,
    /// Schema names derived from the object names (`dbo`, `sales`, ...). Empty for engines whose
    /// objects are not schema-qualified (MySQL/SQLite), which then render categories directly.
    schemas: Vec<String>,
    expanded_schemas: BTreeSet<String>,
    opened_schemas: BTreeSet<String>,
}

#[derive(Clone)]
struct TreeTable {
    name: String,
    /// The leaf label shown in the tree: the bare object name when `name` is
    /// schema-qualified (`users` for `dbo.users`), so it is not redundant under a schema node.
    display: String,
    is_view: bool,
    /// The schema prefix of `name` (`dbo` in `dbo.users`), when present.
    schema: Option<String>,
}

/// Split a schema-qualified object name (`dbo.users`) into `(schema, object)`.
fn split_schema(name: &str) -> Option<(String, &str)> {
    name.split_once('.')
        .filter(|(schema, object)| !schema.is_empty() && !object.is_empty())
        .map(|(schema, object)| (schema.to_string(), object))
}

/// The distinct schema prefixes among the loaded tables/routines. Empty when no object is
/// schema-qualified, so schema-less engines keep the flat database → category tree.
fn build_schema_nodes(
    tables: &Loadable<Vec<TreeTable>>,
    routines: &Loadable<Vec<RoutineInfo>>,
) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    if let Loadable::Loaded(items) = tables {
        for table in items {
            if let Some(schema) = &table.schema {
                names.insert(schema.clone());
            }
        }
    }
    if let Loadable::Loaded(items) = routines {
        for routine in items {
            if let Some((schema, _)) = split_schema(&routine.name) {
                names.insert(schema);
            }
        }
    }
    names.into_iter().collect()
}

/// One saved-query leaf shown under a database's Queries category in the connection tree.
struct TreeSavedQuery {
    /// The database folder the query belongs to, matching a `TreeDatabase`'s name.
    database: String,
    /// The schema folder the query was filed under, when it has one.
    schema: Option<String>,
    /// The query's display name.
    name: String,
    /// The index into `AppView::query_files`, used to open it.
    index: usize,
}

fn snapshot_connections(app: &AppView) -> Vec<TreeConnection> {
    app.connections
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let status = match &node.status {
                ConnectionStatus::Connected(_) => TreeStatus::Connected,
                ConnectionStatus::Connecting => TreeStatus::Connecting,
                ConnectionStatus::Failed(_) => TreeStatus::Failed,
                ConnectionStatus::Disconnected => TreeStatus::Disconnected,
            };
            let failed_error = match &node.status {
                ConnectionStatus::Failed(message) => Some(message.clone()),
                _ => None,
            };
            let databases = match &node.databases {
                Loadable::Idle => Loadable::Idle,
                Loadable::Loading => Loadable::Loading,
                Loadable::Failed(error) => Loadable::Failed(error.clone()),
                Loadable::Loaded(databases) => Loadable::Loaded(
                    databases
                        .iter()
                        .enumerate()
                        .map(|(index, database)| {
                            // Leaf tables are only read when the database is expanded.
                            let tables = if database.expanded {
                                match &database.tables {
                                    Loadable::Idle => Loadable::Idle,
                                    Loadable::Loading => Loadable::Loading,
                                    Loadable::Failed(error) => Loadable::Failed(error.clone()),
                                    Loadable::Loaded(tables) => Loadable::Loaded(
                                        tables
                                            .iter()
                                            .map(|table| {
                                                let (schema, object) =
                                                    match split_schema(&table.name) {
                                                        Some((schema, object)) => {
                                                            (Some(schema), object.to_string())
                                                        }
                                                        None => (None, table.name.clone()),
                                                    };
                                                TreeTable {
                                                    name: table.name.clone(),
                                                    display: object,
                                                    is_view: matches!(
                                                        table.kind,
                                                        rustgrid_core::ObjectKind::View
                                                    ),
                                                    schema,
                                                }
                                            })
                                            .collect(),
                                    ),
                                }
                            } else {
                                Loadable::Idle
                            };
                            // Routine leaves are only read when the Functions category is expanded.
                            let routines = if database.expanded {
                                match &database.routines {
                                    Loadable::Idle => Loadable::Idle,
                                    Loadable::Loading => Loadable::Loading,
                                    Loadable::Failed(error) => Loadable::Failed(error.clone()),
                                    Loadable::Loaded(routines) => {
                                        Loadable::Loaded(routines.clone())
                                    }
                                }
                            } else {
                                Loadable::Idle
                            };
                            // Merge the schemas derived from object names with the database's own
                            // schema list (loaded on open), so an empty/newly created schema
                            // still appears in the tree.
                            let mut schemas = build_schema_nodes(&tables, &routines);
                            if let Some(Loadable::Loaded(loaded)) = &database.schemas {
                                for name in loaded {
                                    if !schemas.contains(name) {
                                        schemas.push(name.clone());
                                    }
                                }
                                schemas.sort();
                            }
                            TreeDatabase {
                                index,
                                name: database.name.clone(),
                                opened: database.opened,
                                expanded: database.expanded,
                                categories: database.categories,
                                tables,
                                routines,
                                schemas,
                                expanded_schemas: database.expanded_schemas.clone(),
                                opened_schemas: database.opened_schemas.clone(),
                            }
                        })
                        .collect(),
                ),
            };
            let descriptor = app
                .registry
                .get(&node.profile.driver)
                .map(|driver| driver.descriptor());
            TreeConnection {
                index,
                id: node.profile.id.clone(),
                name: node.profile.name.clone(),
                icon: descriptor
                    .as_ref()
                    .map_or("icons/connection.svg", |descriptor| descriptor.icon),
                icon_style: descriptor
                    .as_ref()
                    .map_or(DriverIconStyle::Plain, |descriptor| descriptor.icon_style),
                supports_routines: descriptor.as_ref().is_none_or(|descriptor| {
                    descriptor.capabilities.has(DriverCapability::Routines)
                }),
                supports_schemas: descriptor.as_ref().is_some_and(|descriptor| {
                    descriptor.capabilities.has(DriverCapability::Schemas)
                }),
                status,
                failed_error,
                expanded: node.expanded,
                color: app.connection_color(index),
                databases,
                saved_queries: app
                    .query_files
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| file.connection_id == node.profile.id)
                    .map(|(index, file)| TreeSavedQuery {
                        database: file.database.clone(),
                        schema: file.schema.clone(),
                        name: file.name.clone(),
                        index,
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The ids of the tree rows that are currently visible, in draw order, for keyboard navigation.
fn collect_visible_ids(
    entries: &[GroupEntry],
    connections: &[TreeConnection],
    out: &mut Vec<String>,
) {
    for entry in entries {
        match entry {
            GroupEntry::Group(group) => {
                out.push(format!("group-{}", group.id));
                if group.expanded {
                    collect_visible_ids(&group.children, connections, out);
                }
            }
            GroupEntry::Connection(id) => {
                let Some(conn) = connections.iter().find(|conn| &conn.id == id) else {
                    continue;
                };
                out.push(format!("conn-{}", conn.index));
                if !conn.expanded {
                    continue;
                }
                let Loadable::Loaded(databases) = &conn.databases else {
                    continue;
                };
                for db in databases {
                    out.push(format!("db-{}-{}", conn.index, db.index));
                    if !db.expanded {
                        continue;
                    }
                    let schema_engine = !db.schemas.is_empty();
                    if let Loadable::Loaded(tables) = &db.tables {
                        for table in tables {
                            let visible = if schema_engine {
                                table
                                    .schema
                                    .as_ref()
                                    .is_some_and(|schema| db.expanded_schemas.contains(schema))
                            } else {
                                db.categories.tables
                            };
                            if visible {
                                out.push(format!("tbl-{}-{}-{}", conn.index, db.index, table.name));
                            }
                        }
                    }
                }
            }
        }
    }
}

impl TreePane {
    pub(super) fn new(app: WeakEntity<AppView>, cx: &mut Context<'_, Self>) -> Self {
        Self {
            app,
            scroll: ScrollHandle::new(),
            selected: None,
            selected_table: None,
            rename_row: None,
            focus: cx.focus_handle(),
            theme: Theme::dark(),
            visible_ids: Vec::new(),
        }
    }

    pub(super) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Tree keyboard handling: arrow/Home/End navigation, Enter to open, and F2 to rename.
    fn tree_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<'_, Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
            return;
        }
        match keystroke.key.as_str() {
            "up" | "down" | "home" | "end" => {
                if self.visible_ids.is_empty() {
                    return;
                }
                cx.stop_propagation();
                let current = self
                    .selected
                    .as_ref()
                    .and_then(|selected| self.visible_ids.iter().position(|id| id == selected));
                let next = match keystroke.key.as_str() {
                    "up" => current.map(|index| index.saturating_sub(1)).unwrap_or(0),
                    "down" => current
                        .map(|index| (index + 1).min(self.visible_ids.len() - 1))
                        .unwrap_or(0),
                    "home" => 0,
                    "end" => self.visible_ids.len() - 1,
                    _ => return,
                };
                self.selected = Some(self.visible_ids[next].clone());
                self.selected_table = None;
                self.scroll_selected_into_view(cx);
                cx.notify();
            }
            "enter" => {
                if let Some(id) = self.selected.clone() {
                    cx.stop_propagation();
                    self.activate_tree_id(&id, cx);
                }
            }
            _ => {
                if !keystroke.key.eq_ignore_ascii_case("f2") {
                    return;
                }
                // F2 on a group renames the group in place.
                if let Some(id) = self.selected.as_deref()
                    && let Some(rest) = id.strip_prefix("group-")
                    && let Ok(group_id) = rest.parse::<u64>()
                {
                    cx.stop_propagation();
                    let Some(app) = self.app.upgrade() else {
                        return;
                    };
                    app.update(cx, |app, cx| app.begin_group_rename(group_id, window, cx));
                    cx.notify();
                    return;
                }
                let Some((connection_index, database_index, name, is_view)) =
                    self.selected_table.clone()
                else {
                    return;
                };
                cx.stop_propagation();
                let Some(app) = self.app.upgrade() else {
                    return;
                };
                app.update(cx, |app, cx| {
                    app.begin_rename_table(
                        RowPane::Tree,
                        connection_index,
                        database_index,
                        name,
                        is_view,
                        window,
                        cx,
                    );
                    app.notify_rename_pane(RowPane::Tree, cx);
                });
                cx.notify();
            }
        }
    }

    /// Scroll the selected tree row into view after a keyboard move.
    fn scroll_selected_into_view(&mut self, cx: &mut Context<'_, Self>) {
        let Some(selected) = self.selected.as_ref() else {
            return;
        };
        let Some(index) = self.visible_ids.iter().position(|id| id == selected) else {
            return;
        };
        let _ = cx;
        // Rough row offset (22px rows); enough to keep the moved selection on screen.
        let offset = (index as f32 * 22.0 - 40.0).max(0.0);
        self.scroll.set_offset(Point::new(px(0.0), px(offset)));
    }

    /// Enter/click activation for a structural tree row.
    fn activate_tree_id(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        if let Some(rest) = id.strip_prefix("conn-") {
            if let Ok(index) = rest.parse::<usize>() {
                app.update(cx, |app, cx| {
                    let connected = matches!(
                        app.connections.get(index).map(|node| &node.status),
                        Some(ConnectionStatus::Connected(_))
                    );
                    if connected {
                        app.toggle_expand(index, cx);
                    } else {
                        app.connect(index, cx);
                    }
                });
            }
        } else if let Some(rest) = id.strip_prefix("group-") {
            if let Ok(group_id) = rest.parse::<u64>() {
                app.update(cx, |app, cx| app.toggle_group(group_id, cx));
            }
        } else if let Some(rest) = id.strip_prefix("db-") {
            let mut parts = rest.split('-');
            let connection_index = parts.next().and_then(|value| value.parse::<usize>().ok());
            let database_index = parts.next().and_then(|value| value.parse::<usize>().ok());
            if let (Some(connection_index), Some(database_index)) =
                (connection_index, database_index)
            {
                app.update(cx, |app, cx| {
                    app.open_database(connection_index, database_index, cx)
                });
            }
        }
        cx.notify();
    }

    /// Commit an open in-place rename (e.g. before a click moves the selection elsewhere).
    fn commit_pending_rename(&self, cx: &mut Context<'_, Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        app.update(cx, |app, cx| app.submit_rename(cx));
    }

    fn render_connection(
        &self,
        connection: &TreeConnection,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let index = connection.index;
        let conn_id = format!("conn-{index}");
        let selected = self.selected.as_deref() == Some(conn_id.as_str());
        let connected = matches!(connection.status, TreeStatus::Connected);

        let icon_color = match &connection.status {
            TreeStatus::Connected => theme.icon_connection,
            TreeStatus::Connecting => theme.warning,
            TreeStatus::Failed => theme.danger,
            TreeStatus::Disconnected => theme.neutral,
        };
        // A theme-independent badge color so the driver glyph keeps its contrast on both the
        // light and dark backgrounds.
        let badge = match &connection.status {
            TreeStatus::Connected => 0x2e9e5b,
            TreeStatus::Connecting => 0xb58900,
            TreeStatus::Failed => 0xd13438,
            TreeStatus::Disconnected => 0x6b6b6b,
        };

        let click_app = self.app.clone();
        let menu_app = self.app.clone();
        let drag_app = self.app.clone();
        let drag_id = connection.id.clone();
        // The environment colour is painted on the node itself, so it is visible even while the
        // connection is closed (no children to tint) and while the row is selected (a darker shade
        // replaces the generic selection blue).
        let node_tint = connection.color.map(|color| rgba(color.tint(theme, 0x66)));
        let node_selected = connection.color.map(|color| rgba(color.tint(theme, 0x99)));
        let node_hover = connection.color.map(|color| rgba(color.tint(theme, 0x80)));
        let row = div()
            .id(SharedString::from(conn_id.clone()))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl_1()
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .overflow_hidden()
            .when_some(node_tint, |style, bg| style.bg(bg))
            .when(selected, move |style| {
                style
                    .bg(node_selected.unwrap_or(rgba((theme.tree_selected_bg << 8) | 0xff)))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| {
                style.bg(node_hover.unwrap_or(rgba((theme.tree_hover_bg << 8) | 0xff)))
            })
            .on_click(cx.listener(move |this, event, window, cx| {
                this.commit_pending_rename(cx);
                this.selected_table = None;
                window.focus(&this.focus, cx);
                this.selected = Some(format!("conn-{index}"));
                if let Some(app) = this.app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.clear_info_selection();
                        cx.notify();
                    });
                }
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                if double_click {
                    let _ = click_app.update(cx, |app, cx| {
                        let connected = matches!(
                            app.connections.get(index).map(|node| &node.status),
                            Some(ConnectionStatus::Connected(_))
                        );
                        if connected {
                            app.toggle_expand(index, cx);
                        } else {
                            app.connect(index, cx);
                        }
                    });
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    if let Some(app) = drag_app.upgrade() {
                        app.update(cx, |app, _| {
                            app.tree_press_start(
                                TreeDragPayload::Connection(drag_id.clone()),
                                event.position,
                            );
                        });
                    }
                    let _ = this;
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.selected = Some(format!("conn-{index}"));
                    let _ = menu_app.update(cx, |app, cx| {
                        app.context_menu = Some(ContextMenu {
                            target: ContextTarget::Connection(index),
                            position: event.position,
                        });
                        app.move_submenu = None;
                        app.color_submenu = None;
                        cx.notify();
                    });
                }),
            )
            .child(if connected {
                tree_chevron(connection.expanded, theme.chevron)
            } else {
                chevron_spacer()
            })
            .child(tree_driver_icon(
                connection.icon,
                connection.icon_style,
                icon_color,
                badge,
                connected,
            ))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(connection.name.clone()),
            )
            .when(matches!(connection.status, TreeStatus::Connecting), |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(px(11.0))
                        .text_color(rgb(theme.warning))
                        .child(t!("connection.connecting").to_string()),
                )
            });

        // A failed connection keeps its error, surfaced when the row is hovered (the dialog from
        // the attempt is long gone by then).
        let row = match connection.failed_error.clone() {
            Some(error) => row.tooltip(ui::text_tooltip(error)),
            None => row,
        };

        // A connection's environment colour paints a tint behind the connection and every row
        // under it, so the environment is visible at a glance (参考 Navicat). The connection's own
        // row takes a stronger shade; rows keep their own hover/selection background, which paints
        // over this tint.
        let mut body = div().flex().flex_col().w_full();
        if let Some(color) = connection.color {
            body = body.bg(rgba(color.tint(theme, 0x33)));
        }
        let body = body.child(tree_row_with_rect(
            self.app.clone(),
            row,
            format!("conn-{index}"),
        ));

        let mut sub = div().flex().flex_col().w_full();

        if connection.expanded {
            match &connection.databases {
                Loadable::Idle => {}
                Loadable::Loading => {
                    sub = sub.child(tree_message(
                        t!("common.loading").to_string(),
                        26.0,
                        theme.text_muted,
                    ));
                }
                Loadable::Failed(error) => {
                    sub = sub.child(tree_message(error.clone(), 26.0, theme.danger));
                }
                Loadable::Loaded(databases) => {
                    for database in databases {
                        sub = sub.child(self.render_database(
                            index,
                            database,
                            &connection.saved_queries,
                            connection.supports_routines,
                            connection.supports_schemas,
                            cx,
                        ));
                    }
                }
            }
        }

        body.child(sub)
    }

    fn render_database(
        &self,
        connection_index: usize,
        database: &TreeDatabase,
        saved_queries: &[TreeSavedQuery],
        supports_routines: bool,
        supports_schemas: bool,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let database_index = database.index;
        let db_id = format!("db-{connection_index}-{database_index}");
        let selected = self.selected.as_deref() == Some(db_id.as_str());

        let open_app = self.app.clone();
        let chevron_app = self.app.clone();
        let menu_app = self.app.clone();
        let row = div()
            .id(SharedString::from(db_id.clone()))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl(px(18.0))
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, event, window, cx| {
                this.commit_pending_rename(cx);
                this.selected_table = None;
                window.focus(&this.focus, cx);
                this.selected = Some(format!("db-{connection_index}-{database_index}"));
                if let Some(app) = this.app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.clear_info_selection();
                        cx.notify();
                    });
                }
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                if double_click {
                    let _ = open_app.update(cx, |app, cx| {
                        app.open_database(connection_index, database_index, cx);
                    });
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.selected = Some(format!("db-{connection_index}-{database_index}"));
                    let _ = menu_app.update(cx, |app, cx| {
                        app.dump_submenu = None;
                        app.context_menu = Some(ContextMenu {
                            target: ContextTarget::Database {
                                connection_index,
                                database_index,
                            },
                            position: event.position,
                        });
                        cx.notify();
                    });
                }),
            )
            .child(if database.opened {
                div()
                    .id(SharedString::from(format!(
                        "db-toggle-{connection_index}-{database_index}"
                    )))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        let _ = chevron_app.update(cx, |app, cx| {
                            app.open_database(connection_index, database_index, cx);
                        });
                    }))
                    .child(tree_chevron(database.expanded, theme.chevron))
                    .into_any_element()
            } else {
                chevron_spacer()
            })
            .child(tree_icon(
                "icons/database.svg",
                if database.opened {
                    theme.icon_database_active
                } else {
                    theme.icon_database
                },
            ))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(database.name.clone()),
            );

        let mut sub = div().flex().flex_col().w_full();

        if database.expanded {
            match &database.tables {
                Loadable::Idle => {}
                Loadable::Loading => {
                    sub = sub.child(tree_message(
                        t!("common.loading").to_string(),
                        36.0,
                        theme.text_muted,
                    ));
                }
                Loadable::Failed(error) => {
                    sub = sub.child(tree_message(error.clone(), 36.0, theme.danger));
                }
                Loadable::Loaded(_) => {
                    if !supports_schemas || database.schemas.is_empty() {
                        // Schema-less engines (MySQL/SQLite), or a schema engine whose database has
                        // no user schemas yet, keep the flat category list.
                        for category in Category::ALL {
                            if category == Category::Functions && !supports_routines {
                                continue;
                            }
                            sub = sub.child(self.render_category(
                                connection_index,
                                database_index,
                                None,
                                category,
                                database,
                                saved_queries,
                                cx,
                            ));
                        }
                    } else {
                        for schema in &database.schemas {
                            sub = sub.child(self.render_schema(
                                connection_index,
                                database_index,
                                database,
                                schema,
                                saved_queries,
                                supports_routines,
                                cx,
                            ));
                        }
                    }
                }
            }
        }

        div().flex().flex_col().w_full().child(row).child(sub)
    }

    /// One schema node under a database (SQL Server), holding all of the database's categories
    /// for that schema.
    #[allow(clippy::too_many_arguments)]
    fn render_schema(
        &self,
        connection_index: usize,
        database_index: usize,
        database: &TreeDatabase,
        schema: &str,
        saved_queries: &[TreeSavedQuery],
        supports_routines: bool,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let expanded = database.expanded_schemas.contains(schema);
        let schema_id = format!("schema-{connection_index}-{database_index}-{schema}");
        let selected = self.selected.as_deref() == Some(schema_id.as_str());
        let click_id = schema_id.clone();
        let click_name = schema.to_string();
        let app = self.app.clone();
        let menu_app = self.app.clone();
        let menu_schema = schema.to_string();

        let row = div()
            .id(SharedString::from(schema_id))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl(px(36.0))
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, event, window, cx| {
                this.commit_pending_rename(cx);
                this.selected_table = None;
                window.focus(&this.focus, cx);
                this.selected = Some(click_id.clone());
                // A schema opens on a double-click (like a database), not on a single click: a
                // plain click only highlights the row.
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                if double_click {
                    let name = click_name.clone();
                    let _ = app.update(cx, |app, cx| {
                        app.select_schema(connection_index, database_index, name, cx);
                    });
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.selected = Some(format!(
                        "schema-{connection_index}-{database_index}-{menu_schema}"
                    ));
                    let schema = menu_schema.clone();
                    let _ = menu_app.update(cx, |app, cx| {
                        app.context_menu = Some(ContextMenu {
                            target: ContextTarget::Schema {
                                connection_index,
                                database_index,
                                schema,
                            },
                            position: event.position,
                        });
                        cx.notify();
                    });
                }),
            )
            .child(tree_chevron(expanded, theme.chevron))
            .child(tree_icon(
                "icons/database.svg",
                if database.opened_schemas.contains(schema) {
                    theme.icon_database_active
                } else {
                    theme.icon_database
                },
            ))
            .child(div().child(schema.to_string()));

        let mut sub = div().flex().flex_col().w_full();
        if expanded {
            // Every category lives under the schema, so the whole tree reads as
            // database → schema → 表/视图/函数/查询/备份.
            for category in Category::ALL {
                if category == Category::Functions && !supports_routines {
                    continue;
                }
                sub = sub.child(self.render_category(
                    connection_index,
                    database_index,
                    Some(schema),
                    category,
                    database,
                    saved_queries,
                    cx,
                ));
            }
        }

        div().flex().flex_col().w_full().child(row).child(sub)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_category(
        &self,
        connection_index: usize,
        database_index: usize,
        schema: Option<&str>,
        category: Category,
        database: &TreeDatabase,
        saved_queries: &[TreeSavedQuery],
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let expanded = database.categories.get(category);
        let scope = schema.unwrap_or("");
        let cat_id = format!(
            "cat-{connection_index}-{database_index}-{scope}-{}",
            category.id()
        );
        let selected = self.selected.as_deref() == Some(cat_id.as_str());
        // Under a schema the category sits one level deeper.
        let indent = if schema.is_some() { 54.0 } else { 36.0 };
        let label = t!(category.label()).to_string();
        let icon_color = match category {
            Category::Tables => theme.icon_tables,
            Category::Views => theme.icon_views,
            Category::Functions => theme.icon_functions,
            Category::Queries => theme.icon_queries,
            Category::Backups => theme.icon_backups,
        };
        let click_id = cat_id.clone();
        let click_schema = schema.map(str::to_string);
        let app = self.app.clone();
        let menu_app = self.app.clone();
        let menu_schema = schema.map(str::to_string);

        let row = div()
            .id(SharedString::from(cat_id))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl(px(indent))
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.commit_pending_rename(cx);
                this.selected_table = None;
                window.focus(&this.focus, cx);
                this.selected = Some(click_id.clone());
                let schema = click_schema.clone();
                let _ = app.update(cx, |app, cx| {
                    app.toggle_category(connection_index, database_index, schema, category, cx);
                });
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    // Tables offer New Table / Import / Export / Refresh; the other categories
                    // offer Refresh.
                    window.focus(&this.focus, cx);
                    this.selected_table = None;
                    this.selected = Some(format!(
                        "cat-{connection_index}-{database_index}-{}-{}",
                        menu_schema.clone().unwrap_or_default(),
                        category.id()
                    ));
                    let target = if category == Category::Tables {
                        ContextTarget::TableCategory {
                            connection_index,
                            database_index,
                            schema: menu_schema.clone(),
                        }
                    } else {
                        ContextTarget::ObjectCategory {
                            connection_index,
                            database_index,
                            category,
                        }
                    };
                    let _ = menu_app.update(cx, |app, cx| {
                        app.context_menu = Some(ContextMenu {
                            target,
                            position: event.position,
                        });
                        cx.notify();
                    });
                    cx.notify();
                }),
            )
            .child(tree_chevron(expanded, theme.chevron))
            .child(tree_icon(category.icon_path(), icon_color))
            .child(div().child(label));

        let mut sub = div().flex().flex_col().w_full();
        if expanded {
            match category {
                Category::Tables | Category::Views => {
                    if let Loadable::Loaded(tables) = &database.tables {
                        let want_view = category == Category::Views;
                        let mut leaf_index = 0usize;
                        for table in tables.iter() {
                            if table.is_view != want_view {
                                continue;
                            }
                            if let Some(schema) = schema
                                && table.schema.as_deref() != Some(schema)
                            {
                                continue;
                            }
                            sub = sub.child(self.render_table(
                                connection_index,
                                database_index,
                                leaf_index,
                                &database.name,
                                indent + 18.0,
                                table,
                                cx,
                            ));
                            leaf_index += 1;
                        }
                    }
                }
                Category::Functions => match &database.routines {
                    Loadable::Idle | Loadable::Loading => {
                        sub = sub.child(tree_message(
                            t!("common.loading").to_string(),
                            52.0,
                            theme.text_muted,
                        ));
                    }
                    Loadable::Failed(error) => {
                        sub = sub.child(tree_message(error.clone(), 52.0, theme.danger));
                    }
                    Loadable::Loaded(routines) => {
                        for (index, routine) in routines.iter().enumerate() {
                            if let Some(schema) = schema
                                && !split_schema(&routine.name)
                                    .is_some_and(|(candidate, _)| candidate == schema)
                            {
                                continue;
                            }
                            sub = sub.child(self.render_routine(
                                connection_index,
                                database_index,
                                index,
                                &database.name,
                                indent + 18.0,
                                routine,
                                cx,
                            ));
                        }
                    }
                },
                Category::Queries => {
                    for query in saved_queries.iter().filter(|query| {
                        query.database == database.name
                            && match schema {
                                // A schema shows its own queries plus the database-level ones.
                                Some(scope) => {
                                    query.schema.is_none() || query.schema.as_deref() == Some(scope)
                                }
                                None => query.schema.is_none(),
                            }
                    }) {
                        sub = sub.child(self.render_saved_query(
                            connection_index,
                            database_index,
                            query,
                            cx,
                        ));
                    }
                }
                _ => {}
            }
        }

        div().flex().flex_col().w_full().child(row).child(sub)
    }

    /// One stored routine leaf under the connection tree's Functions category.
    #[allow(clippy::too_many_arguments)]
    fn render_routine(
        &self,
        connection_index: usize,
        database_index: usize,
        routine_index: usize,
        database_name: &str,
        indent: f32,
        routine: &RoutineInfo,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        // The routine's schema-qualified name keeps the id unique across schema nodes.
        let _ = routine_index;
        let routine_id = format!("rtn-{connection_index}-{database_index}-{}", routine.name);
        let selected = self.selected.as_deref() == Some(routine_id.as_str());
        let name = routine.name.clone();
        let display = split_schema(&routine.name)
            .map(|(_, object)| object.to_string())
            .unwrap_or_else(|| routine.name.clone());
        let click_name = name.clone();
        let kind = routine.kind;
        let click_id = routine_id.clone();
        let database_name = database_name.to_string();
        let app = self.app.clone();
        let menu_app = self.app.clone();
        let menu_name = routine.name.clone();

        div()
            .id(SharedString::from(routine_id))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl(px(indent))
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, event, window, cx| {
                window.focus(&this.focus, cx);
                this.selected = Some(click_id.clone());
                this.selected_table = None;
                let _ = app.update(cx, |app, cx| {
                    app.set_info_routine(
                        connection_index,
                        database_index,
                        click_name.clone(),
                        kind,
                    );
                    cx.notify();
                });
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                if double_click {
                    let _ = app.update(cx, |app, cx| {
                        app.open_routine_by_name(
                            connection_index,
                            database_name.clone(),
                            click_name.clone(),
                            kind,
                            cx,
                        );
                    });
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus, cx);
                    this.selected = Some(format!(
                        "rtn-{connection_index}-{database_index}-{menu_name}"
                    ));
                    this.selected_table = None;
                    let _ = menu_app.update(cx, |app, cx| {
                        app.context_menu = Some(ContextMenu {
                            target: ContextTarget::Routine {
                                connection_index,
                                database_index,
                                name: menu_name.clone(),
                                kind,
                            },
                            position: event.position,
                        });
                        cx.notify();
                    });
                    cx.notify();
                }),
            )
            .child(tree_icon(
                routine_icon(kind),
                routine_icon_color(kind, theme),
            ))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(display),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table(
        &self,
        connection_index: usize,
        database_index: usize,
        table_index: usize,
        database_name: &str,
        indent: f32,
        table: &TreeTable,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let is_view = table.is_view;
        // The table's schema-qualified name keeps the id unique across schema nodes.
        let _ = table_index;
        let table_id = format!("tbl-{connection_index}-{database_index}-{}", table.name);
        let selected = self.selected.as_deref() == Some(table_id.as_str());
        let table_name = table.name.clone();
        let database_name = database_name.to_string();
        let click_id = table_id.clone();
        let app = self.app.clone();
        let menu_app = self.app.clone();
        let menu_name = table.name.clone();
        // The row being renamed draws the in-place editor instead of its label, so a bare
        // (transparent) editor never ghosts the old name behind the caret.
        let rename = self.rename_row.as_ref().filter(|row| {
            row.connection_index == connection_index
                && row.database_index == database_index
                && row.old_name == table.name
        });
        let editing_here = rename.is_some();
        let label: AnyElement = match rename {
            Some(row) => div()
                .flex_1()
                .min_w(px(0.0))
                .h(px(20.0))
                .child(ui::rename_field(theme, row.input.clone()))
                .into_any_element(),
            None => div()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(table.display.clone())
                .into_any_element(),
        };

        div()
            .id(SharedString::from(table_id))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl(px(indent))
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if editing_here {
                    return;
                }
                this.commit_pending_rename(cx);
                this.selected_table = Some((
                    connection_index,
                    database_index,
                    table_name.clone(),
                    is_view,
                ));
                window.focus(&this.focus, cx);
                this.selected = Some(click_id.clone());
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                let _ = app.update(cx, |app, cx| {
                    if double_click {
                        app.select_table(
                            connection_index,
                            database_name.clone(),
                            table_name.clone(),
                            is_view,
                            cx,
                        );
                    } else {
                        // A single click only selects the table (and updates the info pane);
                        // opening its data grid takes a double-click, like Navicat.
                        app.set_info_table(connection_index, database_index, table_name.clone());
                    }
                });
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if editing_here {
                        return;
                    }
                    this.commit_pending_rename(cx);
                    window.focus(&this.focus, cx);
                    this.selected = Some(format!(
                        "tbl-{connection_index}-{database_index}-{table_index}"
                    ));
                    this.selected_table =
                        Some((connection_index, database_index, menu_name.clone(), is_view));
                    let _ = menu_app.update(cx, |app, cx| {
                        app.context_menu = Some(ContextMenu {
                            target: ContextTarget::Table {
                                connection_index,
                                database_index,
                                name: menu_name.clone(),
                                is_view,
                                pane: RowPane::Tree,
                            },
                            position: event.position,
                        });
                        cx.notify();
                    });
                    cx.notify();
                }),
            )
            .child(chevron_spacer())
            .child(tree_icon(
                if is_view {
                    "icons/views.svg"
                } else {
                    "icons/tables.svg"
                },
                if is_view {
                    theme.icon_views
                } else {
                    theme.icon_tables
                },
            ))
            .child(label)
    }

    /// One saved-query leaf under the connection tree's Queries category. A single click selects
    /// it (and scopes the Queries main tab to its database); a double-click opens it.
    fn render_saved_query(
        &self,
        connection_index: usize,
        database_index: usize,
        query: &TreeSavedQuery,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let theme = self.theme;
        let query_id = format!("qry-{connection_index}-{database_index}-{}", query.index);
        let selected = self.selected.as_deref() == Some(query_id.as_str());
        let name = query.name.clone();
        let click_id = query_id.clone();
        let app = self.app.clone();
        let file_index = query.index;

        div()
            .id(SharedString::from(query_id))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl(px(54.0))
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.commit_pending_rename(cx);
                this.selected_table = None;
                window.focus(&this.focus, cx);
                this.selected = Some(click_id.clone());
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                let _ = app.update(cx, |app, cx| {
                    if double_click {
                        app.open_saved_query(file_index, cx);
                    } else {
                        cx.notify();
                    }
                });
                cx.notify();
            }))
            .child(chevron_spacer())
            .child(tree_icon("icons/queries.svg", theme.icon_queries))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(name),
            )
    }

    /// Render one top-level or nested layout entry (a group or a connection).
    fn render_tree_entry(
        &self,
        entry: &GroupEntry,
        connections: &[TreeConnection],
        group_rename: Option<&(u64, Entity<TextInput>)>,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        match entry {
            GroupEntry::Group(group) => self.render_group(group, connections, group_rename, cx),
            GroupEntry::Connection(id) => match connections.iter().find(|conn| &conn.id == id) {
                Some(conn) => self.render_connection(conn, cx).into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    /// One group row of the connection tree, plus its (nested) children when expanded.
    fn render_group(
        &self,
        group: &ConnectionGroup,
        connections: &[TreeConnection],
        group_rename: Option<&(u64, Entity<TextInput>)>,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let id = group.id;
        let row_id = format!("group-{id}");
        let click_id = row_id.clone();
        let selected = self.selected.as_deref() == Some(row_id.as_str());
        let renaming = group_rename.filter(|(rid, _)| *rid == id);
        let editing_here = renaming.is_some();

        let label: AnyElement = match renaming {
            Some((_, input)) => div()
                .flex_1()
                .min_w(px(0.0))
                .h(px(20.0))
                // Keep clicks on the editor from reaching the row/background, so editing does not
                // immediately steal focus away (and the caret can be placed).
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_this, _event, _window, cx| cx.stop_propagation()),
                )
                .child(ui::rename_field(theme, input.clone()))
                .into_any_element(),
            None => div()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(group.name.clone())
                .into_any_element(),
        };

        let select_app = self.app.clone();
        let toggle_app = self.app.clone();
        let drag_app = self.app.clone();
        let menu_app = self.app.clone();

        let row = div()
            .id(SharedString::from(row_id.clone()))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.0))
            .pl_1()
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .overflow_hidden()
            .when(selected, move |style| {
                style
                    .bg(rgb(theme.tree_selected_bg))
                    .text_color(rgb(theme.tree_selected_text))
            })
            .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
            .on_click(cx.listener(move |this, event, window, cx| {
                if editing_here {
                    return;
                }
                this.commit_pending_rename(cx);
                this.selected_table = None;
                window.focus(&this.focus, cx);
                this.selected = Some(click_id.clone());
                let double_click =
                    matches!(event, ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2);
                if let Some(app) = select_app.upgrade() {
                    app.update(cx, |app, cx| {
                        if double_click {
                            app.toggle_group(id, cx);
                        } else {
                            cx.notify();
                        }
                    });
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    if editing_here {
                        return;
                    }
                    if let Some(app) = drag_app.upgrade() {
                        app.update(cx, |app, _| {
                            app.tree_press_start(TreeDragPayload::Group(id), event.position);
                        });
                    }
                    let _ = this;
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.selected = Some(row_id.clone());
                    this.selected_table = None;
                    if let Some(app) = menu_app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.move_submenu = None;
                            app.context_menu = Some(ContextMenu {
                                target: ContextTarget::Group { id },
                                position: event.position,
                            });
                            cx.notify();
                        });
                    }
                }),
            )
            .child(
                div()
                    .id(SharedString::from(format!("group-toggle-{id}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        let _ = toggle_app.update(cx, |app, cx| app.toggle_group(id, cx));
                    }))
                    .child(tree_chevron(group.expanded, theme.chevron)),
            )
            .child(tree_icon("icons/folder.svg", theme.icon_group))
            .child(label);
        let row = tree_row_with_rect(self.app.clone(), row, format!("group-{id}"));

        let mut body = div().flex().flex_col().w_full().child(row);
        if group.expanded {
            let mut children = div().flex().flex_col().w_full().pl(px(12.0));
            for child in &group.children {
                children =
                    children.child(self.render_tree_entry(child, connections, group_rename, cx));
            }
            body = body.child(children);
        }
        body.into_any_element()
    }
}

impl Render for TreePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let (theme, connections, layout, rename, group_rename) = {
            let Some(app) = self.app.upgrade() else {
                return div().into_any_element();
            };
            let app = app.read(cx);
            (
                app.theme,
                snapshot_connections(app),
                app.tree_layout.clone(),
                app.rename_row(RowPane::Tree),
                app.group_rename
                    .as_ref()
                    .map(|edit| (edit.id, edit.input.clone())),
            )
        };
        self.theme = theme;
        self.rename_row = rename;

        self.visible_ids.clear();
        collect_visible_ids(&layout, &connections, &mut self.visible_ids);

        let mut list = div().flex().flex_col().w_full();
        if layout.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .p_2()
                    .text_color(rgb(theme.text_muted))
                    .child(t!("sidebar.no_connections").to_string()),
            );
        }
        for entry in &layout {
            list =
                list.child(self.render_tree_entry(entry, &connections, group_rename.as_ref(), cx));
        }

        // The drag hit-test reads last frame's rectangles; drop them now so the rows can republish
        // them during this frame's paint. The drop indicator itself is drawn by `AppView` (which
        // re-renders on every drag move), not here.
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, _| app.clear_tree_row_rects());
        }

        let container = div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .min_h(px(0.0))
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex()
                    .flex_col()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .py_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        this.tree_key(event, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                            // Clicking outside the in-place rename commits it (the editor swallows
                            // clicks on itself), then the tree takes focus so F2 works.
                            this.commit_pending_rename(cx);
                            window.focus(&this.focus, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            // Only the blank area reaches here: every row's right-click handler
                            // stops propagation.
                            window.focus(&this.focus, cx);
                            this.selected = None;
                            this.selected_table = None;
                            if let Some(app) = this.app.upgrade() {
                                app.update(cx, |app, cx| {
                                    app.dump_submenu = None;
                                    app.move_submenu = None;
                                    app.color_submenu = None;
                                    app.context_menu = Some(ContextMenu {
                                        target: ContextTarget::TreeBlank,
                                        position: event.position,
                                    });
                                    cx.notify();
                                });
                            }
                        }),
                    )
                    .child(list),
            );
        container.into_any_element()
    }
}

/// Wrap a connection-tree row so it publishes its window-space rectangle for the drag hit test.
fn tree_row_with_rect(
    app: WeakEntity<AppView>,
    row: impl IntoElement + 'static,
    key: String,
) -> AnyElement {
    div()
        .on_children_prepainted(move |bounds, _window, cx| {
            let Some(rect) = bounds.first().copied() else {
                return;
            };
            let _ = app.update(cx, |app, _| app.note_tree_row_rect(key.clone(), rect));
        })
        .child(row)
        .into_any_element()
}

/// Draw the drag-and-drop indicator (a 2px line or a group highlight) in window coordinates.
pub(super) fn tree_drop_indicator_element(hint: groups::TreeDropHint, theme: Theme) -> AnyElement {
    let element = match hint {
        groups::TreeDropHint::Line(bounds) => div()
            .absolute()
            .left(bounds.left())
            .top(bounds.top())
            .w(bounds.size.width)
            .h(px(2.0))
            .bg(rgb(theme.brand))
            .into_any_element(),
        groups::TreeDropHint::Highlight(bounds) => div()
            .absolute()
            .left(bounds.left())
            .top(bounds.top())
            .w(bounds.size.width)
            .h(bounds.size.height)
            .rounded_sm()
            .border_1()
            .border_color(rgb(theme.brand))
            .bg(rgba(0x3b82f622))
            .into_any_element(),
    };
    deferred(element).with_priority(60).into_any_element()
}
