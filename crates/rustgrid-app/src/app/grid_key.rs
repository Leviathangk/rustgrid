//! The dedicated key page for collection keys (`hash` / `set`).
//!
//! A hash or set is a flat bag of field/value (or member) pairs. The SQL grid's WHERE/sort/field
//! chrome and its record toolbar do not apply to it, and the prototype presents it as a key page:
//! a key header bar with the type, TTL, memory and the collection actions, a numbered
//! Field/Value (or Member) table, and a compact pager. The table itself reuses the grid's rows,
//! cell selection and in-place editor, so editing behaves exactly like every other grid.

use super::*;

impl GridView {
    /// The Redis collection type shown on the dedicated key page, when this grid is one.
    pub(super) fn key_page_kind(&self) -> Option<&str> {
        self.state
            .key_status
            .as_ref()
            .and_then(|status| status.engine.as_deref())
            .filter(|kind| matches!(*kind, "hash" | "set"))
    }

    pub(super) fn is_key_page(&self) -> bool {
        self.key_page_kind().is_some()
    }

    /// Give the key page's Field/Member and Value columns the width of the pane instead of the
    /// content-fitted SQL grid widths: the prototype's key table fills the page, with the value
    /// column taking the remaining room. Runs on every key-page render, so a window resize keeps
    /// the table stretched; the widths are mirrored into the grid state because hit-testing and
    /// the shared cell editor read them from there.
    pub(super) fn layout_key_page_columns(&mut self) {
        let pane = f32::from(self.grid_pane_bounds.borrow().size.width);
        let available = if pane > 240.0 {
            pane - GRID_GUTTER_WIDTH
        } else {
            700.0
        };
        self.state.column_widths = match self.state.columns.len() {
            1 => vec![available.max(200.0)],
            2 => {
                let field = (available * 0.35).clamp(180.0, 340.0);
                vec![field, (available - field).max(220.0)]
            }
            _ => return,
        };
    }

    /// The localized header of one key-page column (the driver calls them `field`, `value` and
    /// `member`; the page shows the prototype's Field / Value / Member labels).
    pub(super) fn key_column_label(&self, name: &str) -> String {
        match name {
            "field" => t!("redis.key.field").to_string(),
            "value" => t!("redis.key.value").to_string(),
            "member" => t!("redis.key.member").to_string(),
            other => other.to_string(),
        }
    }

    /// The key page's header bar: type badge, key name, TTL, memory, and the collection actions.
    pub(super) fn render_key_page_bar(
        &self,
        theme: Theme,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let Some(status) = self.state.key_status.as_ref() else {
            return div().into_any_element();
        };
        let kind = status.engine.clone().unwrap_or_default();
        let hash = kind.eq_ignore_ascii_case("hash");
        let ttl = match status.ttl {
            Some(-1) => t!("redis.key.persist").to_string(),
            Some(ttl) => format!("{}: {ttl}s", t!("redis.key.ttl_label")),
            None => String::new(),
        };
        let memory = status.data_length.map(human_size).unwrap_or_default();
        let delete_enabled =
            self.state.editable && self.state.selection.is_some() && !self.state.rows.is_empty();
        let save_enabled = self.state.editable
            && (self.cell_editor.is_some()
                || !self.state.edits.is_empty()
                || !self.inserts.is_empty());
        let add_label = if hash {
            t!("redis.key.add_field").to_string()
        } else {
            t!("redis.key.add_member").to_string()
        };

        let mut bar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(38.0))
            .flex_none()
            .px_3()
            .bg(rgb(theme.toolbar_bg))
            .border_b_1()
            .border_color(rgb(theme.border))
            .text_size(px(12.0))
            .child(super::objects::key_type_badge(&kind, theme))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme.text))
                    .child(self.state.table.clone()),
            );
        if !kind.is_empty() {
            bar = bar.child(
                div()
                    .text_color(rgb(theme.text_muted))
                    .child(kind.to_uppercase()),
            );
        }
        if !ttl.is_empty() {
            bar = bar.child(div().text_color(rgb(theme.text_muted)).child(ttl));
        }
        if !memory.is_empty() {
            bar = bar.child(
                div()
                    .text_color(rgb(theme.text_muted))
                    .child(format!("{}: {memory}", t!("redis.key.memory"))),
            );
        }
        bar = bar
            .child(div().flex_1())
            .child(ui::toolbar_item(
                "key-page-add",
                "icons/add_field.svg",
                add_label,
                self.state.editable,
                theme,
                cx.listener(|this, _event, window, cx| this.add_insert_row(window, cx)),
            ))
            .child(ui::toolbar_item(
                "key-page-delete",
                "icons/delete_field.svg",
                t!("redis.key.delete_selected").to_string(),
                delete_enabled,
                theme,
                cx.listener(|this, _event, _window, cx| this.open_delete_confirm(cx)),
            ))
            .child(ui::popup_button(
                "key-page-save",
                t!("redis.key.save_changes").to_string(),
                save_enabled,
                theme,
                cx.listener(|this, _event, _window, cx| this.save_grid(cx)),
            ))
            .child(ui::toolbar_item(
                "key-page-refresh",
                "icons/refresh.svg",
                t!("common.refresh").to_string(),
                true,
                theme,
                cx.listener(|this, _event, _window, cx| this.refresh(cx)),
            ))
            .when(self.state.loading, |bar| {
                bar.child(self.render_loading_spinner())
            });
        bar.into_any_element()
    }

    /// A compact pager under the key table (the full record toolbar belongs to SQL tables and is
    /// not drawn on a key page).
    pub(super) fn render_key_page_footer(
        &self,
        theme: Theme,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let total = self
            .state
            .total_rows
            .unwrap_or(self.state.rows.len() as u64);
        let page = self.state.page_index + 1;
        let last = self.state.last_page().map(|last| last + 1);
        let at_first = self.state.page_index == 0;
        let at_last = match self.state.last_page() {
            Some(last) => self.state.page_index >= last,
            None => !self.state.has_next(),
        };
        let pager_button = |id: &'static str, icon: &'static str, enabled: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(20.0))
                .h(px(18.0))
                .flex_none()
                .rounded_sm()
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                })
                .opacity(if enabled { 1.0 } else { 0.35 })
                .child(
                    svg()
                        .path(icon)
                        .w(px(10.0))
                        .h(px(10.0))
                        .flex_none()
                        .text_color(rgb(theme.text_muted)),
                )
        };

        let mut prev = pager_button("key-page-prev", "icons/prev.svg", !at_first);
        if !at_first {
            prev = prev.on_click(cx.listener(|this, _event, _window, cx| this.prev_page(cx)));
        }
        let mut next = pager_button("key-page-next", "icons/next.svg", !at_last);
        if !at_last {
            next = next.on_click(cx.listener(|this, _event, _window, cx| this.next_page(cx)));
        }

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .h(px(24.0))
            .flex_none()
            .px_3()
            .bg(rgb(theme.toolbar_bg))
            .border_t_1()
            .border_color(rgb(theme.border))
            .text_size(px(11.5))
            .text_color(rgb(theme.text_muted))
            .child(format!("{total} {}", t!("redis.key.entries")))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(prev)
                    .child(match last {
                        Some(last) => format!("{page} / {last}"),
                        None => page.to_string(),
                    })
                    .child(next),
            )
            .into_any_element()
    }
}
