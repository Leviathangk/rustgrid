use super::design::*;
use super::*;

use gpui::HitboxBehavior;
use rustgrid_core::{
    ColumnDef, DesignerColumnOption, DesignerFieldColumn, DesignerIndexColumn, DesignerOptionField,
    DesignerTab,
};

impl TableDesignView {
    /// Flip one check-box control of the detail panel. The control is a
    /// [`DesignerColumnOption`] so the panel stays engine-driven.
    fn toggle_detail(&mut self, option: DesignerColumnOption, cx: &mut Context<'_, Self>) {
        match option {
            DesignerColumnOption::AutoIncrement => self.toggle_auto_increment(cx),
            DesignerColumnOption::Unsigned => self.toggle_unsigned(cx),
            DesignerColumnOption::Zerofill => self.toggle_zerofill(cx),
            DesignerColumnOption::Sparse => self.toggle_sparse(cx),
            // The two editor rows are not toggles.
            DesignerColumnOption::Default | DesignerColumnOption::Collation => {}
        }
    }
}

impl Render for TableDesignView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        if let Some(app) = self.app.upgrade() {
            self.theme = app.read(cx).theme;
        }
        self.render_design(window, cx)
    }
}

impl TableDesignView {
    fn render_design(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        if self.focus_pending {
            self.focus_pending = false;
            window.focus(&self.focus, cx);
        }
        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .key_context(DESIGNER_CONTEXT)
            .on_action(cx.listener(|this, _: &SaveDesign, _window, cx| this.save(cx)))
            .on_action(cx.listener(|this, _: &NextDesignerCell, window, cx| {
                this.tab_designer_cell(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PrevDesignerCell, window, cx| {
                this.tab_designer_cell(true, window, cx)
            }))
            .bg(rgb(theme.editor_bg))
            .text_size(px(12.0))
            .text_color(rgb(theme.text))
            .child(self.render_toolbar(cx))
            .child(self.render_subtabs(cx));

        root = root.child(self.render_body(cx));

        let root_bounds = self.root_bounds.clone();
        root = root.on_children_prepainted(move |bounds, _window, _cx| {
            if let Some(first) = bounds.first() {
                *root_bounds.borrow_mut() = *first;
            }
        });

        if self.type_combo.is_some() {
            root = root.child(self.render_type_combo(cx));
        }
        if self.index_combo.is_some() {
            root = root.child(self.render_index_combo(cx));
        }
        if self.index_fields_draft.is_some() {
            root = root.child(self.render_index_fields_popup(window, cx));
        }
        if self.unique_key_fields_row.is_some() {
            root = root.child(self.render_unique_key_fields_popup(cx));
        }
        if self.fk_combo.is_some() {
            root = root.child(self.render_fk_combo(cx));
        }
        if self.fk_fields_open {
            root = root.child(self.render_fk_fields_popup(cx));
        }
        if self.fk_ref_open {
            root = root.child(self.render_fk_ref_popup(cx));
        }

        root.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
            this.hscroll_drag(event, cx);
            this.vscroll_drag(event, cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                let h = this.hscroll_grab.take().is_some();
                let v = this.vscroll_grab.take().is_some();
                if h || v {
                    cx.notify();
                }
            }),
        )
        .into_any_element()
    }

    fn render_toolbar(&self, cx: &mut Context<'_, Self>) -> Div {
        let theme = self.theme;
        let fields = self.tab == DesignerTab::Fields && !self.loading;
        let indexes = self.tab == DesignerTab::Indexes && !self.loading;
        let foreign_keys = self.tab == DesignerTab::ForeignKeys && !self.loading;
        let unique_keys = self.tab == DesignerTab::UniqueKeys && !self.loading;
        let checks = self.tab == DesignerTab::Checks && !self.loading;
        let selected = self.selected_field.is_some();
        let can_up = self.selected_field.map(|index| index > 0).unwrap_or(false);
        let can_down = self
            .selected_field
            .map(|index| index + 1 < self.schema.columns.len())
            .unwrap_or(false);
        let can_delete_index = self
            .selected_index
            .and_then(|row| self.schema.indexes.get(row))
            .map(|index| !index.primary)
            .unwrap_or(false);
        let save_enabled = !self.saving && !self.loading;

        let mut bar = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(28.0))
            .flex_none()
            .px_1()
            .bg(rgb(theme.toolbar_bg))
            .border_b_1()
            .border_color(rgb(theme.border))
            .child(ui::toolbar_item(
                "design-save",
                "icons/save.svg",
                t!("design.save").to_string(),
                save_enabled,
                theme,
                cx.listener(|this, _event, _window, cx| this.save(cx)),
            ))
            .child(toolbar_separator(theme));

        if fields {
            bar = bar
                .child(ui::toolbar_item(
                    "design-add-field",
                    "icons/add_field.svg",
                    t!("design.add_field").to_string(),
                    fields,
                    theme,
                    cx.listener(|this, _event, window, cx| this.begin_empty_field(window, cx)),
                ))
                .child(ui::toolbar_item(
                    "design-insert-field",
                    "icons/insert_field.svg",
                    t!("design.insert_field").to_string(),
                    fields && selected,
                    theme,
                    cx.listener(|this, _event, window, cx| this.begin_inserted_field(window, cx)),
                ))
                .child(ui::toolbar_item(
                    "design-delete-field",
                    "icons/delete_field.svg",
                    t!("design.delete_field").to_string(),
                    fields && selected,
                    theme,
                    cx.listener(|this, _event, _window, cx| {
                        this.request_delete(DesignDeleteKind::Fields, cx)
                    }),
                ))
                .child(toolbar_separator(theme))
                .child(ui::toolbar_item(
                    "design-primary-key",
                    "icons/primary_key.svg",
                    t!("design.primary_key").to_string(),
                    fields && selected,
                    theme,
                    cx.listener(|this, _event, _window, cx| this.toggle_primary_key(cx)),
                ))
                .child(toolbar_separator(theme))
                .child(ui::toolbar_item(
                    "design-move-up",
                    "icons/arrow-up.svg",
                    t!("design.move_up").to_string(),
                    fields && can_up,
                    theme,
                    cx.listener(|this, _event, _window, cx| this.move_field(-1, cx)),
                ))
                .child(ui::toolbar_item(
                    "design-move-down",
                    "icons/arrow-down.svg",
                    t!("design.move_down").to_string(),
                    fields && can_down,
                    theme,
                    cx.listener(|this, _event, _window, cx| this.move_field(1, cx)),
                ));
        } else if indexes {
            bar = bar
                .child(ui::toolbar_item(
                    "design-add-index",
                    "icons/add_field.svg",
                    t!("design.add_index").to_string(),
                    indexes,
                    theme,
                    cx.listener(|this, _event, window, cx| this.begin_empty_index(window, cx)),
                ))
                .child(ui::toolbar_item(
                    "design-delete-index",
                    "icons/delete_field.svg",
                    t!("design.delete_index").to_string(),
                    indexes && can_delete_index,
                    theme,
                    cx.listener(|this, _event, _window, cx| {
                        this.request_delete(DesignDeleteKind::Indexes, cx)
                    }),
                ));
        } else if foreign_keys {
            let can_delete_fk = self.selected_fk.is_some();
            bar = bar
                .child(ui::toolbar_item(
                    "design-add-fk",
                    "icons/add_field.svg",
                    t!("design.add_foreign_key").to_string(),
                    foreign_keys,
                    theme,
                    cx.listener(|this, _event, window, cx| {
                        this.begin_empty_foreign_key(window, cx)
                    }),
                ))
                .child(ui::toolbar_item(
                    "design-delete-fk",
                    "icons/delete_field.svg",
                    t!("design.delete_foreign_key").to_string(),
                    foreign_keys && can_delete_fk,
                    theme,
                    cx.listener(|this, _event, _window, cx| {
                        this.request_delete(DesignDeleteKind::ForeignKeys, cx)
                    }),
                ));
        } else if unique_keys {
            let can_delete = self.selected_unique_key.is_some();
            bar = bar
                .child(ui::toolbar_item(
                    "design-add-unique-key",
                    "icons/add_field.svg",
                    t!("design.add_unique_key").to_string(),
                    unique_keys,
                    theme,
                    cx.listener(|this, _event, window, cx| this.begin_empty_unique_key(window, cx)),
                ))
                .child(ui::toolbar_item(
                    "design-delete-unique-key",
                    "icons/delete_field.svg",
                    t!("design.delete_unique_key").to_string(),
                    unique_keys && can_delete,
                    theme,
                    cx.listener(|this, _event, _window, cx| {
                        this.request_delete(DesignDeleteKind::UniqueKeys, cx)
                    }),
                ));
        } else if checks {
            let can_delete = self.selected_check.is_some();
            bar = bar
                .child(ui::toolbar_item(
                    "design-add-check",
                    "icons/add_field.svg",
                    t!("design.add_check").to_string(),
                    checks,
                    theme,
                    cx.listener(|this, _event, window, cx| this.begin_empty_check(window, cx)),
                ))
                .child(ui::toolbar_item(
                    "design-delete-check",
                    "icons/delete_field.svg",
                    t!("design.delete_check").to_string(),
                    checks && can_delete,
                    theme,
                    cx.listener(|this, _event, _window, cx| {
                        this.request_delete(DesignDeleteKind::Checks, cx)
                    }),
                ));
        }

        bar
    }

    fn render_subtabs(&self, cx: &mut Context<'_, Self>) -> Div {
        let theme = self.theme;
        let mut bar = div()
            .flex()
            .flex_row()
            .items_end()
            .gap_1()
            .h(px(26.0))
            .flex_none()
            .px_2()
            .bg(rgb(theme.toolbar_bg))
            .border_b_1()
            .border_color(rgb(theme.border));
        for tab in self.designer.tabs.iter().copied() {
            let active = self.tab == tab;
            bar = bar.child(
                div()
                    .id(tab.id())
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_4()
                    .h(px(24.0))
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .when(active, move |style| {
                        style
                            .bg(rgb(theme.editor_bg))
                            .border_t_1()
                            .border_l_1()
                            .border_r_1()
                            .border_color(rgb(theme.border))
                            .text_color(rgb(theme.text))
                            .font_weight(FontWeight::SEMIBOLD)
                            .mb(px(-1.0))
                    })
                    .when(!active, move |style| {
                        style
                            .bg(rgb(theme.button_bg))
                            .border_1()
                            .border_color(rgb(theme.border))
                            .text_color(rgb(theme.text_muted))
                            .hover(move |style| style.text_color(rgb(theme.text)))
                    })
                    .on_click(
                        cx.listener(move |this, _event, _window, cx| this.select_tab(tab, cx)),
                    )
                    .child(t!(tab.label_key()).to_string()),
            );
        }
        bar
    }

    fn render_body(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let content: AnyElement = if self.loading && self.original.is_none() {
            div()
                .p_3()
                .text_color(rgb(self.theme.text_muted))
                .child(t!("design.loading").to_string())
                .into_any_element()
        } else {
            match self.tab {
                DesignerTab::Fields => self.render_fields(cx),
                DesignerTab::Indexes => self.render_indexes(cx),
                DesignerTab::ForeignKeys => self.render_foreign_keys(cx),
                DesignerTab::UniqueKeys => self.render_unique_keys(cx),
                DesignerTab::Checks => self.render_checks(cx),
                DesignerTab::Options => self.render_options(cx),
                DesignerTab::Comment => self.render_comment(),
                DesignerTab::Sql => self.render_sql(cx),
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_hidden()
            .child(content)
            .into_any_element()
    }

    /// The 预览 tab: the statement Save would run, in a read-only gpui-kit SQL editor.
    ///
    /// The editor is what makes the script usable: SQL syntax colours, a caret so a mouse drag
    /// selects, Ctrl+A / Ctrl+C, and its own scrolling. A plain text run (gpui-base's
    /// `SelectableText`, what the query editor's 信息 error uses) can be dragged over but the
    /// window's Ctrl+C never reaches it, because nothing inside it holds focus.
    fn render_sql(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let sql = self.preview_sql();
        if sql.trim().is_empty() {
            return div()
                .p_3()
                .text_size(px(12.0))
                .text_color(rgb(theme.text_muted))
                .child(t!("design.no_changes").to_string())
                .into_any_element();
        }
        let editor = self
            .preview_editor
            .get_or_insert_with(|| {
                cx.new(|cx| {
                    ui::SqlEditor::new(cx).options(ui::SqlEditorOptions {
                        language: "sql".into(),
                        font_family: "Consolas".into(),
                        font_size: 12.0,
                        line_number: false,
                        readonly: true,
                    })
                })
            })
            .clone();
        editor.update(cx, |editor, cx| editor.set_text(sql, cx));

        div()
            .id("design-sql")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_hidden()
            .bg(rgb(theme.editor_bg))
            .child(editor)
            .into_any_element()
    }

    fn render_fields(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        // 列名 and 类型 lead every engine's field list; the cells after them are the engine's own
        // (长度/小数位/虚拟 for MySQL, 大小/比例/计算列/排序规则/稀疏 for SQL Server).
        let spec_columns = self.designer.field_columns.clone();
        let mut widths = vec![FIELD_NAME_WIDTH, FIELD_TYPE_WIDTH];
        widths.extend(spec_columns.iter().copied().map(field_column_width));
        let content_width: f32 = DESIGN_GUTTER_WIDTH + widths.iter().sum::<f32>();

        let columns = self.schema.columns.clone();
        let selected = self.selected_field;
        let field_rows = self.field_rows.clone();
        let edit = self
            .edit
            .as_ref()
            .map(|editor| (editor.row, editor.column, editor.input.clone()));
        let weak = self.self_weak.clone();
        let combo_anchor = self.combo_anchor.clone();
        let row_widths = widths.clone();

        let list = uniform_list(
            SharedString::from(format!("design-fields-{}", self.id)),
            columns.len().max(1),
            move |range, _window, _cx| {
                let spec_columns = spec_columns.clone();
                range
                    .map(|row| {
                        if row >= columns.len() {
                            return placeholder_row(
                                PlaceholderKind::Field,
                                &row_widths,
                                theme,
                                &weak,
                            );
                        }
                        let column = &columns[row];
                        let row_selected = field_rows.contains(row);
                        let row_active = selected == Some(row);
                        let row_background = if row % 2 == 1 {
                            theme.row_alt_bg
                        } else {
                            theme.editor_bg
                        };

                        let editor_for = |target: FieldColumn| {
                            edit.as_ref()
                                .filter(|(row_index, column_kind, _)| {
                                    *row_index == row && *column_kind == target
                                })
                                .map(|(_, _, input)| input.clone())
                        };
                        let gutter_weak = weak.clone();

                        let mut row_element = div()
                            .flex()
                            .flex_row()
                            .h(px(DESIGN_ROW_HEIGHT))
                            .bg(rgb(row_background))
                            .child(
                                div()
                                    .id(SharedString::from(format!("design-gutter-{row}")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(DESIGN_ROW_HEIGHT))
                                    .w(px(DESIGN_GUTTER_WIDTH))
                                    .flex_none()
                                    .border_r_1()
                                    .border_color(rgb(theme.grid_line))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _window, cx| {
                                            let _ = gutter_weak.update(cx, |view, cx| {
                                                view.select_field_row(row, &event.modifiers, cx)
                                            });
                                        },
                                    )
                                    .when(row_active, |gutter| {
                                        gutter.child(
                                            svg()
                                                .path("icons/row_marker.svg")
                                                .w(px(7.0))
                                                .h(px(7.0))
                                                .flex_none()
                                                .text_color(rgb(theme.primary)),
                                        )
                                    }),
                            )
                            .child(design_edit_cell(
                                SharedString::from(format!("design-name-{row}")),
                                FIELD_NAME_WIDTH,
                                column.name.clone(),
                                theme,
                                row_selected,
                                &weak,
                                row,
                                FieldColumn::Name,
                                editor_for(FieldColumn::Name),
                            ))
                            .child(design_type_cell(
                                SharedString::from(format!("design-type-{row}")),
                                FIELD_TYPE_WIDTH,
                                column.data_type.clone(),
                                theme,
                                false,
                                &weak,
                                row,
                                combo_anchor.clone(),
                            ));
                        for kind in &spec_columns {
                            row_element = row_element.child(design_field_column_cell(
                                *kind,
                                row,
                                column,
                                field_column_width(*kind),
                                theme,
                                &weak,
                                field_column_text(*kind).and_then(editor_for),
                            ));
                        }
                        row_element
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h(px(0.0));

        let header = self.render_field_header(theme, &widths);
        let grid = self.render_grid_scaffold(cx, header, list, content_width);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(grid)
            .child(self.render_detail())
            .into_any_element()
    }

    fn render_field_header(&self, theme: Theme, widths: &[f32]) -> Div {
        let mut labels: Vec<&'static str> = vec!["design.col.name", "design.col.type"];
        labels.extend(
            self.designer
                .field_columns
                .iter()
                .map(|kind| kind.label_key()),
        );
        let mut header = div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(DESIGN_HEADER_HEIGHT))
            .w_full()
            .bg(rgb(theme.header_bg))
            .border_b_1()
            .border_color(rgb(theme.border))
            .child(
                div()
                    .w(px(DESIGN_GUTTER_WIDTH))
                    .h(px(DESIGN_HEADER_HEIGHT))
                    .flex_none()
                    .border_r_1()
                    .border_color(rgb(theme.grid_line)),
            );
        for (index, key) in labels.iter().enumerate() {
            header = header.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(DESIGN_HEADER_HEIGHT))
                    .w(px(widths[index]))
                    .flex_none()
                    .px_2()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(rgb(theme.grid_line))
                    .child(t!(*key).to_string()),
            );
        }
        header
    }

    /// The per-column detail panel. Its controls come from the engine's
    /// [`TableDesignerSpec::column_options`], so MySQL/MariaDB show 自动递增 / 无符号 / 填充零 while
    /// SQL Server shows 排序规则 / 稀疏, and every engine gets 默认. A control the engine cannot use
    /// on the selected column (SQL Server's `IDENTITY` on a column the table already has, an
    /// auto-increment on a text column) is left out rather than shown dead.
    fn render_detail(&self) -> Div {
        let theme = self.theme;
        let enabled = self.selected_field.is_some();
        let column = self
            .selected_field
            .and_then(|index| self.schema.columns.get(index));
        // Where the engine accepts an auto-increment: on a type of its list, and — for an engine
        // that cannot change a column the table already has (SQL Server's `IDENTITY`) — only on a
        // column the table does not have yet, or in a brand-new table.
        let rule = &self.designer.auto_increment;
        let column_is_new = column
            .map(|column| {
                self.original.as_ref().is_none_or(|original| {
                    !original
                        .columns
                        .iter()
                        .any(|existing| existing.name == column.name)
                })
            })
            .unwrap_or(false);
        let identity_editable = column
            .map(|column| {
                (rule.change_existing || self.is_new || column_is_new)
                    && (column.auto_increment || rule.allows(&column.data_type))
            })
            .unwrap_or(false);

        let mut panel = div()
            .flex()
            .flex_col()
            .flex_none()
            .h(px(DESIGN_DETAIL_HEIGHT))
            .w_full()
            .px_2()
            .py_2()
            .gap_2()
            .bg(rgb(theme.dialog_face))
            .border_t_1()
            .border_color(rgb(theme.border));

        for option in self.designer.column_options.iter().copied() {
            // A control the engine cannot use on the selected column is left out entirely rather
            // than shown dead: SQL Server's `IDENTITY` (one numeric column per table, and only on a
            // new one), or MySQL's `AUTO_INCREMENT` on a type it cannot carry.
            if option == DesignerColumnOption::AutoIncrement && !identity_editable {
                continue;
            }
            let row = match option {
                DesignerColumnOption::Default => detail_editor_row(
                    t!(option.label_key()),
                    div()
                        .w(px(360.0))
                        .h(px(22.0))
                        .when(!enabled, |style| style.opacity(0.5))
                        .child(self.default_input.clone())
                        .into_any_element(),
                ),
                DesignerColumnOption::Collation => detail_editor_row(
                    t!(option.label_key()),
                    div()
                        .w(px(360.0))
                        .h(px(22.0))
                        .when(!enabled, |style| style.opacity(0.5))
                        .child(self.column_collation_combo.clone())
                        .into_any_element(),
                ),
                _ => {
                    let checked = match option {
                        DesignerColumnOption::AutoIncrement => {
                            column.map(|column| column.auto_increment).unwrap_or(false)
                        }
                        DesignerColumnOption::Unsigned => {
                            column.map(|column| column.unsigned).unwrap_or(false)
                        }
                        DesignerColumnOption::Zerofill => {
                            column.map(|column| column.zerofill).unwrap_or(false)
                        }
                        DesignerColumnOption::Sparse => {
                            column.map(|column| column.sparse).unwrap_or(false)
                        }
                        _ => false,
                    };
                    detail_toggle_row(option, checked, enabled, theme, self.self_weak.clone())
                }
            };
            panel = panel.child(row);
        }

        panel
    }

    fn render_indexes(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        // The grid's columns come from the engine: its own 索引类型, the 索引方法 cell only when its
        // kinds carry more than one type (MySQL/MariaDB's BTREE/HASH), 注释 only when its indexes
        // carry one.
        let columns = index_grid_columns(&self.designer);
        let widths: Vec<f32> = columns.iter().map(|(_, width)| *width).collect();
        let show_method = columns.iter().any(|(key, _)| *key == "design.index.method");
        let show_comment = columns
            .iter()
            .any(|(key, _)| *key == "design.index.comment");
        let content_width: f32 = DESIGN_GUTTER_WIDTH + widths.iter().sum::<f32>();
        let indexes = self.schema.indexes.clone();
        let selected = self.selected_index;
        let index_rows = self.index_rows.clone();
        let designer = self.designer.clone();
        let edit = self
            .index_edit
            .as_ref()
            .map(|editor| (editor.row, editor.column, editor.input.clone()));
        let weak = self.self_weak.clone();
        let combo_anchor = self.index_combo_anchor.clone();
        let fields_anchor = self.index_fields_anchor.clone();

        let list = uniform_list(
            SharedString::from(format!("design-indexes-{}", self.id)),
            indexes.len().max(1),
            move |range, _window, _cx| {
                range
                    .map(|row| {
                        if row >= indexes.len() {
                            return placeholder_row(PlaceholderKind::Index, &widths, theme, &weak);
                        }
                        let index = &indexes[row];
                        let row_selected = index_rows.contains(row);
                        let row_active = selected == Some(row);
                        let row_background = if row % 2 == 1 {
                            theme.row_alt_bg
                        } else {
                            theme.editor_bg
                        };
                        let editor_for = |target: IndexColumn| {
                            edit.as_ref()
                                .filter(|(row_index, column_kind, _)| {
                                    *row_index == row && *column_kind == target
                                })
                                .map(|(_, _, input)| input.clone())
                        };
                        let kind_label = index_kind_label(&designer, index);
                        let method = index_method_label(&designer, index);
                        let gutter_weak = weak.clone();

                        let mut line = div()
                            .flex()
                            .flex_row()
                            .h(px(DESIGN_ROW_HEIGHT))
                            .bg(rgb(row_background))
                            .child(
                                div()
                                    .id(SharedString::from(format!("index-gutter-{row}")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(DESIGN_ROW_HEIGHT))
                                    .w(px(DESIGN_GUTTER_WIDTH))
                                    .flex_none()
                                    .border_r_1()
                                    .border_color(rgb(theme.grid_line))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _window, cx| {
                                            let _ = gutter_weak.update(cx, |view, cx| {
                                                view.select_index_row(row, &event.modifiers, cx)
                                            });
                                        },
                                    )
                                    .when(row_active, |gutter| {
                                        gutter.child(
                                            svg()
                                                .path("icons/row_marker.svg")
                                                .w(px(7.0))
                                                .h(px(7.0))
                                                .flex_none()
                                                .text_color(rgb(theme.primary)),
                                        )
                                    }),
                            )
                            .child(index_edit_cell(
                                SharedString::from(format!("index-name-{row}")),
                                INDEX_NAME_WIDTH,
                                index.name.clone(),
                                theme,
                                row_selected,
                                &weak,
                                row,
                                IndexColumn::Name,
                                editor_for(IndexColumn::Name),
                            ))
                            .child(design_picker_cell(
                                SharedString::from(format!("index-fields-{row}")),
                                INDEX_FIELDS_WIDTH,
                                index
                                    .columns
                                    .iter()
                                    .map(|column| column.display())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                                theme,
                                // The primary key row is editable here too: its columns are the
                                // ones flagged 主键 in the 字段 tab.
                                true,
                                DesignPicker::IndexFields,
                                &weak,
                                row,
                                fields_anchor.clone(),
                            ))
                            .child(index_choice_cell(
                                SharedString::from(format!("index-kind-{row}")),
                                INDEX_KIND_WIDTH,
                                kind_label,
                                !index.primary,
                                theme,
                                false,
                                &weak,
                                row,
                                IndexComboField::Kind,
                                combo_anchor.clone(),
                            ));
                        if show_method {
                            line = line.child(index_choice_cell(
                                SharedString::from(format!("index-method-{row}")),
                                INDEX_METHOD_WIDTH,
                                method.unwrap_or_default().to_string(),
                                !index.primary && method.is_some(),
                                theme,
                                false,
                                &weak,
                                row,
                                IndexComboField::Method,
                                combo_anchor.clone(),
                            ));
                        }
                        if show_comment {
                            line = line.child(index_edit_cell(
                                SharedString::from(format!("index-comment-{row}")),
                                INDEX_COMMENT_WIDTH,
                                index.comment.clone(),
                                theme,
                                false,
                                &weak,
                                row,
                                IndexColumn::Comment,
                                editor_for(IndexColumn::Comment),
                            ));
                        }
                        line
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h(px(0.0));

        let header = self.render_index_header(theme, &columns);

        self.render_grid_scaffold(cx, header, list, content_width)
    }

    fn render_index_header(&self, theme: Theme, columns: &[(&'static str, f32)]) -> Div {
        let mut header = div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(DESIGN_HEADER_HEIGHT))
            .w_full()
            .bg(rgb(theme.header_bg))
            .border_b_1()
            .border_color(rgb(theme.border))
            .child(
                div()
                    .w(px(DESIGN_GUTTER_WIDTH))
                    .h(px(DESIGN_HEADER_HEIGHT))
                    .flex_none()
                    .border_r_1()
                    .border_color(rgb(theme.grid_line)),
            );
        for (key, width) in columns {
            header = header.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(DESIGN_HEADER_HEIGHT))
                    .w(px(*width))
                    .flex_none()
                    .px_2()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(rgb(theme.grid_line))
                    .child(t!(*key).to_string()),
            );
        }
        header
    }

    fn render_foreign_keys(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let widths = [
            FK_NAME_WIDTH,
            FK_COLUMNS_WIDTH,
            FK_SCHEMA_WIDTH,
            FK_REF_TABLE_WIDTH,
            FK_REF_COLUMNS_WIDTH,
            FK_DELETE_WIDTH,
            FK_UPDATE_WIDTH,
        ];
        let content_width: f32 = DESIGN_GUTTER_WIDTH + widths.iter().sum::<f32>();
        let foreign_keys = self.schema.foreign_keys.clone();
        let database = self.database.clone();
        let selected = self.selected_fk;
        let fk_rows = self.fk_rows.clone();
        let edit = self
            .fk_edit
            .as_ref()
            .map(|editor| (editor.row, editor.column, editor.input.clone()));
        let weak = self.self_weak.clone();
        let combo_anchor = self.fk_combo_anchor.clone();
        let fields_anchor = self.fk_fields_anchor.clone();
        let ref_anchor = self.fk_ref_anchor.clone();

        let list = uniform_list(
            SharedString::from(format!("design-fks-{}", self.id)),
            foreign_keys.len().max(1),
            move |range, _window, _cx| {
                range
                    .map(|row| {
                        if row >= foreign_keys.len() {
                            return placeholder_row(
                                PlaceholderKind::ForeignKey,
                                &widths,
                                theme,
                                &weak,
                            );
                        }
                        let foreign_key = &foreign_keys[row];
                        let row_selected = fk_rows.contains(row);
                        let row_active = selected == Some(row);
                        let row_background = if row % 2 == 1 {
                            theme.row_alt_bg
                        } else {
                            theme.editor_bg
                        };
                        let editor_for = |target: FkColumn| {
                            edit.as_ref()
                                .filter(|(row_index, column_kind, _)| {
                                    *row_index == row && *column_kind == target
                                })
                                .map(|(_, _, input)| input.clone())
                        };
                        let gutter_weak = weak.clone();

                        div()
                            .flex()
                            .flex_row()
                            .h(px(DESIGN_ROW_HEIGHT))
                            .bg(rgb(row_background))
                            .child(
                                div()
                                    .id(SharedString::from(format!("fk-gutter-{row}")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(DESIGN_ROW_HEIGHT))
                                    .w(px(DESIGN_GUTTER_WIDTH))
                                    .flex_none()
                                    .border_r_1()
                                    .border_color(rgb(theme.grid_line))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _window, cx| {
                                            let _ = gutter_weak.update(cx, |view, cx| {
                                                view.select_fk_row(row, &event.modifiers, cx)
                                            });
                                        },
                                    )
                                    .when(row_active, |gutter| {
                                        gutter.child(
                                            svg()
                                                .path("icons/row_marker.svg")
                                                .w(px(7.0))
                                                .h(px(7.0))
                                                .flex_none()
                                                .text_color(rgb(theme.primary)),
                                        )
                                    }),
                            )
                            .child(fk_edit_cell(
                                SharedString::from(format!("fk-name-{row}")),
                                FK_NAME_WIDTH,
                                foreign_key.name.clone(),
                                theme,
                                row_selected,
                                &weak,
                                row,
                                FkColumn::Name,
                                editor_for(FkColumn::Name),
                            ))
                            .child(design_picker_cell(
                                SharedString::from(format!("fk-columns-{row}")),
                                FK_COLUMNS_WIDTH,
                                foreign_key.columns.join(", "),
                                theme,
                                true,
                                DesignPicker::ForeignKeyColumns,
                                &weak,
                                row,
                                fields_anchor.clone(),
                            ))
                            .child(
                                design_cell(
                                    SharedString::from(format!("fk-schema-{row}")),
                                    FK_SCHEMA_WIDTH,
                                    theme,
                                    false,
                                )
                                .px_2()
                                .child(database.clone()),
                            )
                            .child(fk_edit_cell(
                                SharedString::from(format!("fk-ref-table-{row}")),
                                FK_REF_TABLE_WIDTH,
                                foreign_key.referenced_table.clone(),
                                theme,
                                false,
                                &weak,
                                row,
                                FkColumn::ReferencedTable,
                                editor_for(FkColumn::ReferencedTable),
                            ))
                            .child(design_picker_cell(
                                SharedString::from(format!("fk-ref-columns-{row}")),
                                FK_REF_COLUMNS_WIDTH,
                                foreign_key.referenced_columns.join(", "),
                                theme,
                                true,
                                DesignPicker::ForeignKeyReferenced,
                                &weak,
                                row,
                                ref_anchor.clone(),
                            ))
                            .child(fk_choice_cell(
                                SharedString::from(format!("fk-delete-{row}")),
                                FK_DELETE_WIDTH,
                                rule_label(&foreign_key.on_delete),
                                theme,
                                false,
                                &weak,
                                row,
                                FkComboField::OnDelete,
                                combo_anchor.clone(),
                            ))
                            .child(fk_choice_cell(
                                SharedString::from(format!("fk-update-{row}")),
                                FK_UPDATE_WIDTH,
                                rule_label(&foreign_key.on_update),
                                theme,
                                false,
                                &weak,
                                row,
                                FkComboField::OnUpdate,
                                combo_anchor.clone(),
                            ))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h(px(0.0));

        let header = self.render_fk_header(theme, &widths);

        self.render_grid_scaffold(cx, header, list, content_width)
    }

    fn render_fk_header(&self, theme: Theme, widths: &[f32; 7]) -> Div {
        let labels = [
            "design.fk.name",
            "design.fk.columns",
            "design.fk.schema",
            "design.fk.ref_table",
            "design.fk.ref_columns",
            "design.fk.delete",
            "design.fk.update",
        ];
        let mut header = div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(DESIGN_HEADER_HEIGHT))
            .w_full()
            .bg(rgb(theme.header_bg))
            .border_b_1()
            .border_color(rgb(theme.border))
            .child(
                div()
                    .w(px(DESIGN_GUTTER_WIDTH))
                    .h(px(DESIGN_HEADER_HEIGHT))
                    .flex_none()
                    .border_r_1()
                    .border_color(rgb(theme.grid_line)),
            );
        for (index, key) in labels.iter().enumerate() {
            header = header.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(DESIGN_HEADER_HEIGHT))
                    .w(px(widths[index]))
                    .flex_none()
                    .px_2()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(rgb(theme.grid_line))
                    .child(t!(*key).to_string()),
            );
        }
        header
    }

    /// The 唯一键 grid. SQL Server keeps `UNIQUE` constraints apart from indexes, so this tab lists
    /// them; an engine that reports uniqueness through 索引 does not declare the tab at all.
    fn render_unique_keys(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let widths = [UQ_NAME_WIDTH, UQ_FIELDS_WIDTH];
        let content_width: f32 = DESIGN_GUTTER_WIDTH + widths.iter().sum::<f32>();
        let unique_keys = self.schema.unique_keys.clone();
        let selected = self.selected_unique_key;
        let rows = self.unique_key_rows.clone();
        let edit = self
            .unique_key_edit
            .as_ref()
            .map(|editor| (editor.row, editor.column, editor.input.clone()));
        let weak = self.self_weak.clone();
        let fields_anchor = self.unique_key_fields_anchor.clone();

        let list = uniform_list(
            SharedString::from(format!("design-unique-keys-{}", self.id)),
            unique_keys.len().max(1),
            move |range, _window, _cx| {
                range
                    .map(|row| {
                        if row >= unique_keys.len() {
                            return placeholder_row(
                                PlaceholderKind::UniqueKey,
                                &widths,
                                theme,
                                &weak,
                            );
                        }
                        let key = &unique_keys[row];
                        let row_selected = rows.contains(row);
                        let row_active = selected == Some(row);
                        let row_background = if row % 2 == 1 {
                            theme.row_alt_bg
                        } else {
                            theme.editor_bg
                        };
                        let editor = edit
                            .as_ref()
                            .filter(|(row_index, column_kind, _)| {
                                *row_index == row && *column_kind == UniqueKeyColumn::Name
                            })
                            .map(|(_, _, input)| input.clone());
                        let gutter_weak = weak.clone();

                        div()
                            .flex()
                            .flex_row()
                            .h(px(DESIGN_ROW_HEIGHT))
                            .bg(rgb(row_background))
                            .child(
                                div()
                                    .id(SharedString::from(format!("uq-gutter-{row}")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(DESIGN_ROW_HEIGHT))
                                    .w(px(DESIGN_GUTTER_WIDTH))
                                    .flex_none()
                                    .border_r_1()
                                    .border_color(rgb(theme.grid_line))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _window, cx| {
                                            let _ = gutter_weak.update(cx, |view, cx| {
                                                view.select_unique_key_row(
                                                    row,
                                                    &event.modifiers,
                                                    cx,
                                                )
                                            });
                                        },
                                    )
                                    .when(row_active, |gutter| {
                                        gutter.child(
                                            svg()
                                                .path("icons/row_marker.svg")
                                                .w(px(7.0))
                                                .h(px(7.0))
                                                .flex_none()
                                                .text_color(rgb(theme.primary)),
                                        )
                                    }),
                            )
                            .child(unique_key_edit_cell(
                                SharedString::from(format!("uq-name-{row}")),
                                UQ_NAME_WIDTH,
                                key.name.clone(),
                                theme,
                                row_selected,
                                &weak,
                                row,
                                editor,
                            ))
                            .child(design_picker_cell(
                                SharedString::from(format!("uq-fields-{row}")),
                                UQ_FIELDS_WIDTH,
                                key.columns.join(", "),
                                theme,
                                true,
                                DesignPicker::UniqueKeyFields,
                                &weak,
                                row,
                                fields_anchor.clone(),
                            ))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h(px(0.0));

        let header = design_grid_header(theme, &["design.uq.name", "design.uq.fields"], &widths);

        self.render_grid_scaffold(cx, header, list, content_width)
    }

    /// The 检查 grid: check constraints, edited as name + expression.
    fn render_checks(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let widths = [CHECK_NAME_WIDTH, CHECK_EXPRESSION_WIDTH];
        let content_width: f32 = DESIGN_GUTTER_WIDTH + widths.iter().sum::<f32>();
        let checks = self.schema.checks.clone();
        let selected = self.selected_check;
        let rows = self.check_rows.clone();
        let edit = self
            .check_edit
            .as_ref()
            .map(|editor| (editor.row, editor.column, editor.input.clone()));
        let weak = self.self_weak.clone();

        let list = uniform_list(
            SharedString::from(format!("design-checks-{}", self.id)),
            checks.len().max(1),
            move |range, _window, _cx| {
                range
                    .map(|row| {
                        if row >= checks.len() {
                            return placeholder_row(PlaceholderKind::Check, &widths, theme, &weak);
                        }
                        let check = &checks[row];
                        let row_selected = rows.contains(row);
                        let row_active = selected == Some(row);
                        let row_background = if row % 2 == 1 {
                            theme.row_alt_bg
                        } else {
                            theme.editor_bg
                        };
                        let editor_for = |target: CheckColumn| {
                            edit.as_ref()
                                .filter(|(row_index, column_kind, _)| {
                                    *row_index == row && *column_kind == target
                                })
                                .map(|(_, _, input)| input.clone())
                        };
                        let gutter_weak = weak.clone();

                        div()
                            .flex()
                            .flex_row()
                            .h(px(DESIGN_ROW_HEIGHT))
                            .bg(rgb(row_background))
                            .child(
                                div()
                                    .id(SharedString::from(format!("check-gutter-{row}")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(DESIGN_ROW_HEIGHT))
                                    .w(px(DESIGN_GUTTER_WIDTH))
                                    .flex_none()
                                    .border_r_1()
                                    .border_color(rgb(theme.grid_line))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _window, cx| {
                                            let _ = gutter_weak.update(cx, |view, cx| {
                                                view.select_check_row(row, &event.modifiers, cx)
                                            });
                                        },
                                    )
                                    .when(row_active, |gutter| {
                                        gutter.child(
                                            svg()
                                                .path("icons/row_marker.svg")
                                                .w(px(7.0))
                                                .h(px(7.0))
                                                .flex_none()
                                                .text_color(rgb(theme.primary)),
                                        )
                                    }),
                            )
                            .child(check_edit_cell(
                                SharedString::from(format!("check-name-{row}")),
                                CHECK_NAME_WIDTH,
                                check.name.clone(),
                                theme,
                                row_selected,
                                &weak,
                                row,
                                CheckColumn::Name,
                                editor_for(CheckColumn::Name),
                            ))
                            .child(check_edit_cell(
                                SharedString::from(format!("check-expression-{row}")),
                                CHECK_EXPRESSION_WIDTH,
                                check.expression.clone(),
                                theme,
                                false,
                                &weak,
                                row,
                                CheckColumn::Expression,
                                editor_for(CheckColumn::Expression),
                            ))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h(px(0.0));

        let header = design_grid_header(
            theme,
            &["design.check.name", "design.check.expression"],
            &widths,
        );

        self.render_grid_scaffold(cx, header, list, content_width)
    }

    /// The scroll container a designer grid lives in: the header and list scroll horizontally with
    /// the view's own scrollbars, and the wheel drives both axes.
    ///
    /// A gpui `uniform_list` scrolls itself from the *bubble* phase of a `ScrollWheelEvent`, which
    /// the grids' surrounding scrollable containers also take part in, so the designer's lists did
    /// not scroll under the wheel. This owns a hitbox in the capture phase instead (exactly as the
    /// table grid does): the wheel scrolls the rows, a horizontal gesture pans the columns, and a
    /// gesture aimed at an overlay above the grid is left alone (`should_handle_scroll`).
    fn render_grid_scaffold(
        &self,
        cx: &mut Context<'_, Self>,
        header: Div,
        list: impl IntoElement,
        content_width: f32,
    ) -> AnyElement {
        let table = div()
            .id("design-hscroll")
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .track_scroll(&self.hscroll)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .h_full()
                    .w(px(content_width))
                    .child(header)
                    .child(list),
            );

        let weak = self.self_weak.clone();
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .child(table)
                            .child(self.render_hscrollbar(cx)),
                    )
                    .child(self.render_vscrollbar(cx)),
            )
            .child(
                canvas(
                    |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |_bounds, hitbox, window, _cx| {
                        window.on_mouse_event(
                            move |event: &ScrollWheelEvent, phase, window, cx| {
                                if phase != DispatchPhase::Capture
                                    || !hitbox.should_handle_scroll(window)
                                {
                                    return;
                                }
                                let (dx, dy) = match event.delta {
                                    ScrollDelta::Lines(delta) => {
                                        (delta.x * DESIGN_ROW_HEIGHT, delta.y * DESIGN_ROW_HEIGHT)
                                    }
                                    ScrollDelta::Pixels(delta) => {
                                        (f32::from(delta.x), f32::from(delta.y))
                                    }
                                };
                                if dx == 0.0 && dy == 0.0 {
                                    return;
                                }
                                let consumed = weak
                                    .update(cx, |view, cx| view.design_scroll(dx, dy, cx))
                                    .unwrap_or(false);
                                if consumed {
                                    cx.stop_propagation();
                                }
                            },
                        );
                    },
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }

    /// The table's storage options. The rows come from the engine's
    /// [`TableDesignerSpec::option_fields`], so an engine only shows what its designer SQL honors.
    fn render_options(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        self.sync_option_combos(cx);
        let mut panel = div().flex().flex_col().gap_3().p_4();
        for field in self.designer.option_fields.iter().copied() {
            let row = match field {
                DesignerOptionField::Engine => option_combo_row(
                    t!(field.label_key()),
                    Some(self.engine_combo.clone()),
                    theme,
                ),
                DesignerOptionField::Charset => option_combo_row(
                    t!(field.label_key()),
                    Some(self.charset_combo.clone()),
                    theme,
                ),
                DesignerOptionField::Collation => option_combo_row(
                    t!(field.label_key()),
                    Some(self.collation_combo.clone()),
                    theme,
                ),
                DesignerOptionField::AutoIncrement => option_row(
                    t!(field.label_key()),
                    self.auto_increment_input.clone(),
                    theme,
                ),
                DesignerOptionField::Comment => {
                    option_row(t!(field.label_key()), self.comment_input.clone(), theme)
                }
            };
            panel = panel.child(row);
        }
        panel = panel.child(div().flex_1());
        panel.into_any_element()
    }

    fn render_comment(&self) -> AnyElement {
        let theme = self.theme;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(option_row(
                t!("design.comment_label"),
                self.comment_input.clone(),
                theme,
            ))
            .child(div().flex_1())
            .into_any_element()
    }

    fn render_type_combo(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(row) = self.type_combo else {
            return div().into_any_element();
        };
        let origin = self.root_bounds.borrow().origin;
        let anchor = self
            .combo_anchor
            .borrow()
            .get(&row)
            .copied()
            .unwrap_or_default();
        let anchor = Point::new(anchor.x - origin.x, anchor.y - origin.y);
        let current = self
            .schema
            .columns
            .get(row)
            .map(|column| column.data_type.clone())
            .unwrap_or_default();
        let weak = self.self_weak.clone();

        let mut options = div()
            .id("design-type-options")
            .max_h(px(300.0))
            .overflow_y_scroll()
            .track_scroll(&self.type_scroll)
            .flex()
            .flex_col();
        for (index, data_type) in self.filtered_types().into_iter().enumerate() {
            let selected = data_type == current;
            let highlighted = self.type_highlight == Some(index);
            let weak = weak.clone();
            options = options.child(
                div()
                    .id(SharedString::from(format!(
                        "design-type-option-{data_type}"
                    )))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .h(px(TYPE_OPTION_HEIGHT))
                    .px_2()
                    .flex_none()
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .when(selected, move |style| {
                        style
                            .bg(rgb(theme.tree_selected_bg))
                            .text_color(rgb(theme.tree_selected_text))
                    })
                    // The keyboard highlight (Up/Down) reads like a hover, and outranks the
                    // selected row's tint so the row the caret is on is the one that stands out.
                    .when(highlighted, move |style| {
                        style
                            .bg(rgb(theme.tree_hover_bg))
                            .text_color(rgb(theme.text))
                    })
                    .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    .on_click(move |_event, _window, cx| {
                        let _ = weak.update(cx, |view, cx| view.select_type(row, data_type, cx));
                    })
                    .child(data_type.to_string())
                    .child(
                        svg()
                            .path("icons/check.svg")
                            .w(px(12.0))
                            .h(px(12.0))
                            .flex_none()
                            .text_color(rgb(theme.tree_selected_text))
                            .opacity(if selected { 1.0 } else { 0.0 }),
                    ),
            );
        }

        let mut popup = ui::popup_panel(theme)
            .id("design-type-list")
            .left(anchor.x)
            .top(anchor.y)
            .w(px(FIELD_TYPE_WIDTH))
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.type_combo = None;
                cx.notify();
            }));
        if let Some(search) = self.type_search.clone() {
            popup = popup.child(
                div()
                    .flex_none()
                    .p_1()
                    .border_b_1()
                    .border_color(rgb(theme.grid_line))
                    .child(div().h(px(20.0)).child(search)),
            );
        }
        popup = popup.child(options);
        deferred(popup).with_priority(100).into_any_element()
    }

    /// The dropdown popup for an index row's kind/method cells.
    fn render_index_combo(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some((row, field)) = self.index_combo else {
            return div().into_any_element();
        };
        let Some(index) = self.schema.indexes.get(row) else {
            return div().into_any_element();
        };
        let origin = self.root_bounds.borrow().origin;
        let anchor = self
            .index_combo_anchor
            .borrow()
            .get(&(row, field))
            .copied()
            .unwrap_or_default();
        let anchor = Point::new(anchor.x - origin.x, anchor.y - origin.y);
        let weak = self.self_weak.clone();

        let mut options = div()
            .id("design-index-options")
            .max_h(px(300.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let width = match field {
            IndexComboField::Kind => {
                let current = self
                    .designer
                    .index_kind(index)
                    .map(|kind| kind.label)
                    .unwrap_or_default();
                for (position, kind) in self.designer.index_kinds.iter().enumerate() {
                    options = options.child(index_choice_option(
                        // The label carries spaces on SQL Server (`UNIQUE NONCLUSTERED`), so the
                        // element id is positional rather than built from it.
                        format!("index-kind-option-{position}"),
                        kind.label.to_string(),
                        kind.label == current,
                        row,
                        field,
                        kind.label,
                        &weak,
                        theme,
                    ));
                }
                INDEX_KIND_WIDTH
            }
            IndexComboField::Method => {
                let current = index_method_label(&self.designer, index);
                for method in self.designer.index_methods().unwrap_or_default() {
                    options = options.child(index_choice_option(
                        format!("index-method-option-{method}"),
                        method.to_string(),
                        current == Some(*method),
                        row,
                        field,
                        method,
                        &weak,
                        theme,
                    ));
                }
                INDEX_METHOD_WIDTH
            }
        };

        let popup = ui::popup_panel(theme)
            .id("design-index-list")
            .left(anchor.x)
            .top(anchor.y)
            .w(px(width))
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.index_combo = None;
                cx.notify();
            }));
        deferred(popup.child(options))
            .with_priority(100)
            .into_any_element()
    }

    /// Open the field drop-down a picker cell owns (the whole cell is the trigger).
    pub(super) fn open_design_picker(
        &mut self,
        row: usize,
        picker: DesignPicker,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match picker {
            DesignPicker::IndexFields => self.open_index_fields(row, window, cx),
            DesignPicker::ForeignKeyColumns => self.open_fk_fields(row, window, cx),
            DesignPicker::ForeignKeyReferenced => self.open_fk_ref(row, window, cx),
            DesignPicker::UniqueKeyFields => self.open_unique_key_fields(row, window, cx),
        }
    }

    /// The index's 字段 editor: its columns in order, each with the engine's per-column options
    /// (排序 for every engine, 子部分 for MySQL/MariaDB), reordered with ↑ ↓ and removed with −.
    ///
    /// Everything edits a draft; 确定 installs it and 取消 (or a click outside) throws it away, so the
    /// grid never shows a half-made change. The 主键 row opens here too — its draft edits the 主键
    /// flags, which is where the model keeps the key — and therefore carries no per-column options.
    ///
    /// The panel is sized from its own columns — the grid's 字段 column is narrower than 字段 + 排序 +
    /// 子部分, which is what used to clip 子部分 — and from the room the view has: it opens under the
    /// cell, or above it when that is where the space is, and the list takes whatever height is left
    /// so the footer's 取消 / 确定 are never pushed out of the window.
    fn render_index_fields_popup(
        &self,
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let Some(draft) = self.index_fields_draft.as_ref() else {
            return div().into_any_element();
        };
        let bounds = *self.root_bounds.borrow();
        let origin = bounds.origin;
        let anchor = self
            .index_fields_anchor
            .borrow()
            .get(&draft.row)
            .copied()
            .unwrap_or_default();
        let anchor = Point::new(anchor.x - origin.x, anchor.y - origin.y);
        let anchor_y: f32 = anchor.y.into();
        let view_height: f32 = bounds.size.height.into();
        let options = !draft.primary;
        let sort = options
            && self
                .designer
                .index_columns
                .contains(&DesignerIndexColumn::Sort);
        let prefix = options
            && self
                .designer
                .index_columns
                .contains(&DesignerIndexColumn::PrefixLength);
        let picker_open = self.index_field_picker.is_some();
        let width = index_fields_panel_width(&self.designer, draft.primary);
        let reserved = INDEX_FIELDS_CHROME_HEIGHT
            + if picker_open {
                INDEX_FIELDS_PICKER_HEIGHT
            } else {
                0.0
            };
        let below = view_height - anchor_y - reserved;
        let above = anchor_y - DESIGN_ROW_HEIGHT - reserved;
        let open_up = below < INDEX_FIELDS_LIST_MIN_HEIGHT && above > below;
        let list_height = if open_up { above } else { below }
            .clamp(INDEX_FIELDS_LIST_MIN_HEIGHT, INDEX_FIELDS_LIST_HEIGHT);

        let mut panel = ui::popup_panel(theme)
            .id("design-index-fields")
            .left(anchor.x)
            .w(px(width))
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.close_index_fields(cx);
            }));
        if open_up {
            panel = panel.bottom(px(view_height - anchor_y + DESIGN_ROW_HEIGHT));
        } else {
            panel = panel.top(anchor.y);
        }

        let mut header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .flex_none()
            .border_b_1()
            .border_color(rgb(theme.border))
            .text_color(rgb(theme.text_muted))
            .child(
                div()
                    .w(px(INDEX_FIELD_NAME_WIDTH))
                    .flex_none()
                    .child(t!("design.index.fields").to_string()),
            );
        for option in [DesignerIndexColumn::Sort, DesignerIndexColumn::PrefixLength] {
            if (option == DesignerIndexColumn::Sort && sort)
                || (option == DesignerIndexColumn::PrefixLength && prefix)
            {
                header = header.child(
                    div()
                        .w(px(INDEX_FIELD_OPTION_WIDTH))
                        .flex_none()
                        .child(t!(option.label_key()).to_string()),
                );
            }
        }
        panel = panel.child(header);

        let mut list = div()
            .id("design-index-fields-list")
            .flex()
            .flex_col()
            .flex_none()
            .min_h(px(24.0))
            .max_h(px(list_height))
            .overflow_y_scroll();
        for (row, column) in draft.columns.iter().enumerate() {
            let selected = draft.selected == Some(row);
            // The name cell is the row's drop-down: clicking it opens the panel's field list (an
            // in-panel list rather than a floating menu, which this panel would treat as a click
            // outside and close) with a search box to find the column by typing.
            let expanded = self.index_field_picker == Some(row);
            let mut name_cell = div()
                .id(SharedString::from(format!("index-field-name-{row}")))
                .flex()
                .flex_row()
                .items_center()
                .w(px(INDEX_FIELD_NAME_WIDTH))
                .h(px(20.0))
                .flex_none()
                .rounded(px(3.0))
                .border_1()
                .border_color(rgb(theme.border))
                .bg(rgb(theme.input_bg))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _event, window, cx| {
                    this.toggle_index_field_picker(row, window, cx)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .px_1()
                        .overflow_hidden()
                        .child(column.name.clone()),
                )
                .child(
                    svg()
                        .path("icons/chevron-down.svg")
                        .w(px(10.0))
                        .h(px(10.0))
                        .flex_none()
                        .mr_1()
                        .text_color(rgb(theme.text_muted)),
                );
            if expanded {
                name_cell = name_cell.border_color(rgb(theme.primary));
            }
            let mut line = div()
                .id(SharedString::from(format!("index-field-row-{row}")))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(26.0))
                .flex_none()
                .cursor_pointer()
                .when(selected, |style| style.bg(rgb(theme.tree_selected_bg)))
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.select_index_draft_row(row, cx)
                }))
                .child(name_cell);
            if sort {
                let descending = column.descending;
                line = line.child(
                    div()
                        .id(SharedString::from(format!("index-field-sort-{row}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(INDEX_FIELD_OPTION_WIDTH))
                        .h(px(18.0))
                        .flex_none()
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(rgb(theme.border))
                        .bg(rgb(theme.input_bg))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.toggle_index_draft_sort(row, cx)
                        }))
                        .child(if descending { "DESC" } else { "ASC" }.to_string()),
                );
            }
            if prefix {
                let cell: AnyElement = match self.index_field_prefix_inputs.get(&row) {
                    Some(input) => div()
                        .w(px(INDEX_FIELD_OPTION_WIDTH))
                        .h(px(20.0))
                        .flex_none()
                        .child(input.clone())
                        .into_any_element(),
                    None => div().w(px(INDEX_FIELD_OPTION_WIDTH)).into_any_element(),
                };
                line = line.child(cell);
            }
            list = list.child(line);
        }
        panel = panel.child(list);

        if self.index_field_picker.is_some() {
            // The field list sits under the rows as the panel's own sub-list. It is not nested in
            // the rows' scroller — a wheel event reaches *every* scrollable hitbox under the cursor,
            // so nesting one inside another would scroll both.
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .border_b_1()
                    .border_color(rgb(theme.border))
                    .bg(rgb(theme.dialog_bg))
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .border_b_1()
                            .border_color(rgb(theme.border))
                            .child({
                                let search: AnyElement = match self.index_field_search.clone() {
                                    Some(input) => {
                                        div().h(px(20.0)).child(input).into_any_element()
                                    }
                                    None => div().h(px(20.0)).into_any_element(),
                                };
                                search
                            }),
                    )
                    .child({
                        let mut options = div()
                            .id("design-index-field-options")
                            .flex()
                            .flex_col()
                            .flex_none()
                            .max_h(px(INDEX_FIELDS_PICKER_HEIGHT - INDEX_FIELDS_SEARCH_HEIGHT))
                            .overflow_y_scroll();
                        for (row, column) in draft.columns.iter().enumerate() {
                            if self.index_field_picker != Some(row) {
                                continue;
                            }
                            for name in self.index_field_options() {
                                let picked = name == column.name;
                                let option_name = name.clone();
                                options = options.child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "index-field-option-{row}-{name}"
                                        )))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .gap_1()
                                        .h(px(20.0))
                                        .px_2()
                                        .flex_none()
                                        .cursor_pointer()
                                        .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                                        .on_click(cx.listener(move |this, _event, _window, cx| {
                                            this.set_index_draft_name(row, &option_name, cx)
                                        }))
                                        .child(name)
                                        .child(
                                            svg()
                                                .path("icons/check.svg")
                                                .w(px(12.0))
                                                .h(px(12.0))
                                                .flex_none()
                                                .text_color(rgb(theme.tree_selected_text))
                                                .opacity(if picked { 1.0 } else { 0.0 }),
                                        ),
                                );
                            }
                        }
                        options
                    }),
            );
        }

        let mut footer = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .flex_none()
            .border_t_1()
            .border_color(rgb(theme.border));
        for (id, label, delta) in [
            ("design-index-field-up", "↑", -1isize),
            ("design-index-field-down", "↓", 1),
        ] {
            footer = footer.child(ui::popup_button(
                id,
                label.to_string(),
                false,
                theme,
                cx.listener(move |this, _event, _window, cx| {
                    this.move_index_draft_column(delta, cx)
                }),
            ));
        }
        footer = footer
            .child(ui::popup_button(
                "design-index-field-add",
                "+".to_string(),
                false,
                theme,
                cx.listener(|this, _event, _window, cx| this.add_index_draft_column(cx)),
            ))
            .child(ui::popup_button(
                "design-index-field-remove",
                "−".to_string(),
                false,
                theme,
                cx.listener(|this, _event, _window, cx| this.remove_index_draft_column(cx)),
            ))
            .child(div().flex_1())
            .child(ui::popup_button(
                "design-index-fields-cancel",
                t!("form.cancel").to_string(),
                false,
                theme,
                cx.listener(|this, _event, _window, cx| this.close_index_fields(cx)),
            ))
            .child(ui::popup_button(
                "design-index-fields-ok",
                t!("form.ok").to_string(),
                true,
                theme,
                cx.listener(|this, _event, _window, cx| this.confirm_index_fields(cx)),
            ));
        panel = panel.child(footer);

        deferred(panel).with_priority(100).into_any_element()
    }

    /// The "choose fields" popup for one unique key.
    fn render_unique_key_fields_popup(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some(row) = self.unique_key_fields_row else {
            return div().into_any_element();
        };
        let Some(key) = self.schema.unique_keys.get(row) else {
            return div().into_any_element();
        };
        let origin = self.root_bounds.borrow().origin;
        let anchor = self
            .unique_key_fields_anchor
            .borrow()
            .get(&row)
            .copied()
            .unwrap_or_default();
        let anchor = Point::new(anchor.x - origin.x, anchor.y - origin.y);
        let current = key.columns.clone();
        let columns = self.schema.columns.clone();
        let weak = self.self_weak.clone();

        let mut list = div()
            .id("design-unique-key-fields-list")
            .max_h(px(260.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for column in &columns {
            let name = column.name.clone();
            let option_name = name.clone();
            let checked = current.iter().any(|existing| existing == &name);
            let weak = weak.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("uq-field-option-{name}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(20.0))
                    .px_2()
                    .flex_none()
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    .on_click(move |_event, _window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.toggle_unique_key_column(row, &option_name, cx)
                        });
                    })
                    .child(checkbox_box(checked, theme))
                    .child(name),
            );
        }

        let popup = ui::popup_panel(theme)
            .id("design-unique-key-fields")
            .left(anchor.x)
            .top(anchor.y)
            .w(px(UQ_FIELDS_WIDTH))
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.unique_key_fields_row = None;
                cx.notify();
            }));
        deferred(popup.child(list))
            .with_priority(100)
            .into_any_element()
    }

    /// The dropdown popup for a foreign-key row's on-delete/on-update cells.
    fn render_fk_combo(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let Some((row, field)) = self.fk_combo else {
            return div().into_any_element();
        };
        if self.schema.foreign_keys.get(row).is_none() {
            return div().into_any_element();
        }
        let origin = self.root_bounds.borrow().origin;
        let anchor = self
            .fk_combo_anchor
            .borrow()
            .get(&(row, field))
            .copied()
            .unwrap_or_default();
        let anchor = Point::new(anchor.x - origin.x, anchor.y - origin.y);
        let weak = self.self_weak.clone();
        let current = match field {
            FkComboField::OnDelete => self.schema.foreign_keys[row].on_delete.as_str(),
            FkComboField::OnUpdate => self.schema.foreign_keys[row].on_update.as_str(),
        };

        let mut options = div()
            .id("design-fk-options")
            .max_h(px(300.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for rule in FK_RULES {
            options = options.child(fk_choice_option(
                format!("fk-rule-option-{}", rule.replace(' ', "_")),
                rule_label(rule),
                rule == current,
                row,
                field,
                rule,
                &weak,
                theme,
            ));
        }

        let popup = ui::popup_panel(theme)
            .id("design-fk-list")
            .left(anchor.x)
            .top(anchor.y)
            .w(px(FK_DELETE_WIDTH.max(FK_UPDATE_WIDTH)))
            .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                this.fk_combo = None;
                cx.notify();
            }));
        deferred(popup.child(options))
            .with_priority(100)
            .into_any_element()
    }

    fn render_fk_fields_popup(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let columns: Vec<String> = self
            .schema
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect();
        self.render_fk_picker_popup(columns, true, cx)
    }

    fn render_fk_ref_popup(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let columns = self.fk_ref_columns.clone();
        self.render_fk_picker_popup(columns, false, cx)
    }

    /// Shared renderer for the foreign key's child/referenced column pickers. `child` selects which
    /// list the toggles write to.
    fn render_fk_picker_popup(
        &self,
        columns: Vec<String>,
        child: bool,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let Some(row) = self.selected_fk else {
            return div().into_any_element();
        };
        let Some(foreign_key) = self.schema.foreign_keys.get(row) else {
            return div().into_any_element();
        };
        let selected: Vec<String> = if child {
            foreign_key.columns.clone()
        } else {
            foreign_key.referenced_columns.clone()
        };
        let origin = self.root_bounds.borrow().origin;
        let anchor = if child {
            self.fk_fields_anchor.borrow().get(&row).copied()
        } else {
            self.fk_ref_anchor.borrow().get(&row).copied()
        }
        .unwrap_or_default();
        let anchor = Point::new(anchor.x - origin.x, anchor.y - origin.y);
        let weak = self.self_weak.clone();

        let mut list = div()
            .id("design-fk-fields-list")
            .max_h(px(260.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if columns.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_size(px(12.0))
                    .text_color(rgb(theme.text_muted))
                    .child(t!("design.fk.no_columns").to_string()),
            );
        }
        for column in &columns {
            let name = column.clone();
            let option_name = name.clone();
            let checked = selected.iter().any(|existing| existing == &name);
            let weak = weak.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("fk-field-option-{name}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(20.0))
                    .px_2()
                    .flex_none()
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
                    .on_click(move |_event, _window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            if child {
                                view.toggle_fk_column(row, &option_name, cx)
                            } else {
                                view.toggle_fk_ref_column(row, &option_name, cx)
                            }
                        });
                    })
                    .child(checkbox_box(checked, theme))
                    .child(name),
            );
        }

        let popup = ui::popup_panel(theme)
            .id("design-fk-fields")
            .left(anchor.x)
            .top(anchor.y)
            .w(px(if child {
                FK_COLUMNS_WIDTH
            } else {
                FK_REF_COLUMNS_WIDTH
            }))
            .on_mouse_down_out(cx.listener(move |this, _event, _window, cx| {
                if child {
                    this.fk_fields_open = false;
                } else {
                    this.fk_ref_open = false;
                }
                cx.notify();
            }));
        deferred(popup.child(list))
            .with_priority(100)
            .into_any_element()
    }

    fn render_hscrollbar(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let viewport = f32::from(self.hscroll.bounds().size.width);
        let max = f32::from(self.hscroll.max_offset().x);
        if max <= 0.0 {
            return div().flex_none().h(px(14.0)).into_any_element();
        }
        let scroll = -f32::from(self.hscroll.offset().x);
        let (thumb_left, thumb_len) = scrollbar_fractions(viewport, max, scroll);
        ui::hscrollbar_track("design-hscrollbar", theme, thumb_left, thumb_len)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.hscroll_begin(event.position.x, cx);
                }),
            )
            .into_any_element()
    }

    fn render_vscrollbar(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = self.theme;
        let handle = self.list_scroll.0.borrow().base_handle.clone();
        let (viewport, max) = self.design_scroll_extent();
        if max <= 0.0 {
            return div().into_any_element();
        }
        let scroll = -f32::from(handle.offset().y);
        let (thumb_top, thumb_len) = scrollbar_fractions(viewport, max, scroll);
        div()
            .flex_none()
            .h_full()
            .pt(px(DESIGN_HEADER_HEIGHT))
            .pb(px(14.0))
            .child(
                ui::vscrollbar_track("design-vscrollbar", theme, thumb_top, thumb_len)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            this.vscroll_begin(event.position.y, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Apply one wheel delta to the designer's grids, the same way the table grid does.
    ///
    /// A `uniform_list` registers **no** scroll-wheel handler of its own (unlike gpui's `list`), so
    /// its shared scroll offset cannot simply be written: the wheel has to move the selected row and
    /// bring it into view through `scroll_to_item`, which is the only scrolling a uniform list
    /// performs — and is exactly what the table grid's wheel does. `dx` pans the columns through the
    /// plain scroll handle of the surrounding container.
    pub(super) fn design_scroll(&mut self, dx: f32, dy: f32, cx: &mut Context<'_, Self>) -> bool {
        let mut consumed = false;
        if dy != 0.0 {
            consumed = self.wheel_selected_row(dy, cx);
        }
        if dx != 0.0 {
            let max = f32::from(self.hscroll.max_offset().x);
            if max > 0.0 {
                let scroll = -f32::from(self.hscroll.offset().x);
                let next = (scroll - dx).clamp(0.0, max);
                if (next - scroll).abs() >= 0.5 {
                    let y = self.hscroll.offset().y;
                    self.hscroll.set_offset(Point::new(px(-next), y));
                }
                // A horizontal gesture is claimed even at the end of the columns: gpui maps a
                // delta with no room on its axis onto the other one, so letting it bubble would
                // roll the rows during a swipe past the last column.
                consumed = true;
            }
        }
        if consumed {
            cx.notify();
        }
        consumed
    }

    /// Move the active tab's row selection by one row — towards the first row for a positive
    /// `delta` (a wheel-up notch) — and scroll it into view. Returns whether the wheel was used.
    fn wheel_selected_row(&mut self, delta: f32, cx: &mut Context<'_, Self>) -> bool {
        let rows = self.design_row_count();
        if rows == 0 {
            return false;
        }
        // Scrolling ends an in-place edit, exactly as it does in the table grid: the editor's text
        // is already in the working copy, and a focused editor must not scroll out of view.
        self.clear_editors();
        // One notch moves a single row, whatever the OS reports for it.
        let step: isize = if delta > 0.0 { -1 } else { 1 };
        let current = self.selected_design_row();
        let Some(current) = current else {
            // Nothing selected yet: the first notch lands on the first row, so the wheel can start
            // walking through the grid (the table grid does the same).
            self.select_design_row(0, cx);
            self.list_scroll.scroll_to_item(0, ScrollStrategy::Nearest);
            return true;
        };
        let next = (current as isize + step).clamp(0, rows as isize - 1) as usize;
        if next == current {
            // Already on the first or the last row: leave the event to whatever is behind the grid.
            return false;
        }
        self.select_design_row(next, cx);
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        true
    }

    /// The active tab's selected row, if any.
    fn selected_design_row(&self) -> Option<usize> {
        match self.tab {
            DesignerTab::Fields => self.selected_field,
            DesignerTab::Indexes => self.selected_index,
            DesignerTab::ForeignKeys => self.selected_fk,
            DesignerTab::UniqueKeys => self.selected_unique_key,
            DesignerTab::Checks => self.selected_check,
            DesignerTab::Options | DesignerTab::Comment | DesignerTab::Sql => None,
        }
    }

    /// Select one row of the active tab's grid (gutter selection included, as a gutter click does).
    fn select_design_row(&mut self, row: usize, cx: &mut Context<'_, Self>) {
        match self.tab {
            DesignerTab::Fields => self.select_field(row, cx),
            DesignerTab::Indexes => self.select_index(row, cx),
            DesignerTab::ForeignKeys => self.select_fk(row, cx),
            DesignerTab::UniqueKeys => self.select_unique_key(row, cx),
            DesignerTab::Checks => self.select_check(row, cx),
            DesignerTab::Options | DesignerTab::Comment | DesignerTab::Sql => {}
        }
    }

    fn hscroll_begin(&mut self, mouse_x: Pixels, cx: &mut Context<'_, Self>) {
        let bounds = self.hscroll.bounds();
        let viewport = f32::from(bounds.size.width);
        let max = f32::from(self.hscroll.max_offset().x);
        let (thumb_w, travel) = scrollbar_thumb(viewport, max);
        if travel <= 0.0 {
            return;
        }
        let scroll = -f32::from(self.hscroll.offset().x);
        let thumb_x = (scroll / max) * travel;
        let relative = f32::from(mouse_x) - f32::from(bounds.left());
        let grab = if relative >= thumb_x && relative <= thumb_x + thumb_w {
            relative - thumb_x
        } else {
            thumb_w / 2.0
        };
        self.hscroll_grab = Some(grab);
        self.hscroll_set(relative, grab, cx);
    }

    fn hscroll_set(&self, relative: f32, grab: f32, cx: &mut Context<'_, Self>) {
        let viewport = f32::from(self.hscroll.bounds().size.width);
        let max = f32::from(self.hscroll.max_offset().x);
        let (_, travel) = scrollbar_thumb(viewport, max);
        if travel <= 0.0 {
            return;
        }
        let thumb_x = (relative - grab).clamp(0.0, travel);
        let scroll = thumb_x / travel * max;
        let y = self.hscroll.offset().y;
        self.hscroll.set_offset(Point::new(px(-scroll), y));
        cx.notify();
    }

    /// The number of rows the grid on the active tab holds (fields, indexes, foreign keys, unique
    /// keys, checks); a tab without a grid has none.
    pub(super) fn design_row_count(&self) -> usize {
        match self.tab {
            DesignerTab::Fields => self.schema.columns.len(),
            DesignerTab::Indexes => self.schema.indexes.len(),
            DesignerTab::ForeignKeys => self.schema.foreign_keys.len(),
            DesignerTab::UniqueKeys => self.schema.unique_keys.len(),
            DesignerTab::Checks => self.schema.checks.len(),
            DesignerTab::Options | DesignerTab::Comment | DesignerTab::Sql => 0,
        }
    }

    /// Bring one row of the active tab's grid into view.
    ///
    /// A `uniform_list` only ever scrolls through `scroll_to_item` (see [`Self::design_scroll`]), so a
    /// row added at the end by 添加字段 / 添加索引 has to ask to be revealed itself. `Nearest` turns
    /// into a scroll-to-bottom for a row below the viewport, which is applied on the list's next
    /// prepaint — by then it knows the new row count, so the grid lands on the row just added.
    pub(super) fn reveal_design_row(&self, row: usize) {
        self.list_scroll
            .scroll_to_item(row, ScrollStrategy::Nearest);
    }

    /// The row list's scroll extent as `(viewport_height, max_scroll)`.
    ///
    /// A `uniform_list` never reports its content size through `ScrollHandle::max_offset` — the
    /// table grid computes the same extent from its row count for exactly this reason — so an
    /// extent read from the handle is always zero, which is why the designer's vertical scrollbar
    /// never appeared and neither the wheel nor its thumb could move the list.
    fn design_scroll_extent(&self) -> (f32, f32) {
        let handle = self.list_scroll.0.borrow().base_handle.clone();
        let viewport = f32::from(handle.bounds().size.height);
        let max = (self.design_row_count() as f32 * DESIGN_ROW_HEIGHT - viewport).max(0.0);
        (viewport, max)
    }

    fn vscroll_begin(&mut self, mouse_y: Pixels, cx: &mut Context<'_, Self>) {
        let handle = self.list_scroll.0.borrow().base_handle.clone();
        let bounds = handle.bounds();
        let (viewport, max) = self.design_scroll_extent();
        let (thumb_h, travel) = scrollbar_thumb(viewport, max);
        if travel <= 0.0 {
            return;
        }
        let scroll = -f32::from(handle.offset().y);
        let thumb_y = (scroll / max) * travel;
        let relative = f32::from(mouse_y) - f32::from(bounds.top());
        let grab = if relative >= thumb_y && relative <= thumb_y + thumb_h {
            relative - thumb_y
        } else {
            thumb_h / 2.0
        };
        self.vscroll_grab = Some(grab);
        self.vscroll_set(relative, grab, cx);
    }

    fn vscroll_set(&self, relative: f32, grab: f32, cx: &mut Context<'_, Self>) {
        let (viewport, max) = self.design_scroll_extent();
        let (_, travel) = scrollbar_thumb(viewport, max);
        if travel <= 0.0 {
            return;
        }
        let thumb_y = (relative - grab).clamp(0.0, travel);
        let scroll = thumb_y / travel * max;
        // A uniform list scrolls by item, not by arbitrary pixels, so the thumb lands on the row
        // that sits at that position — the closest its own scrolling can get.
        let row = (scroll / DESIGN_ROW_HEIGHT).round() as usize;
        self.list_scroll
            .scroll_to_item_strict(row, ScrollStrategy::Top);
        cx.notify();
    }

    pub(super) fn hscroll_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<'_, Self>) {
        let Some(grab) = self.hscroll_grab else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.hscroll_grab = None;
            cx.notify();
            return;
        }
        let relative = f32::from(event.position.x) - f32::from(self.hscroll.bounds().left());
        self.hscroll_set(relative, grab, cx);
    }

    pub(super) fn vscroll_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<'_, Self>) {
        let Some(grab) = self.vscroll_grab else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.vscroll_grab = None;
            cx.notify();
            return;
        }
        let handle = self.list_scroll.0.borrow().base_handle.clone();
        let relative = f32::from(event.position.y) - f32::from(handle.bounds().top());
        self.vscroll_set(relative, grab, cx);
    }
}

/// The grid whose blank placeholder row is being rendered. Clicking it creates the first entry.
#[derive(Clone, Copy)]
enum PlaceholderKind {
    Field,
    Index,
    ForeignKey,
    UniqueKey,
    Check,
}

/// A blank editable row shown below the header when a designer grid is empty. Clicking any cell
/// creates the first entry and starts editing its name, so an empty grid is never a dead end.
fn placeholder_row(
    kind: PlaceholderKind,
    widths: &[f32],
    theme: Theme,
    weak: &WeakEntity<TableDesignView>,
) -> Div {
    let prefix = match kind {
        PlaceholderKind::Field => "design-field",
        PlaceholderKind::Index => "design-index",
        PlaceholderKind::ForeignKey => "design-fk",
        PlaceholderKind::UniqueKey => "design-uq",
        PlaceholderKind::Check => "design-check",
    };
    let mut row = div()
        .flex()
        .flex_row()
        .h(px(DESIGN_ROW_HEIGHT))
        .bg(rgb(theme.editor_bg))
        .child(
            div()
                .w(px(DESIGN_GUTTER_WIDTH))
                .h(px(DESIGN_ROW_HEIGHT))
                .flex_none()
                .border_r_1()
                .border_color(rgb(theme.grid_line)),
        );
    for (index, width) in widths.iter().enumerate() {
        let weak = weak.clone();
        row = row.child(
            design_cell(
                SharedString::from(format!("{prefix}-empty-{index}")),
                *width,
                theme,
                false,
            )
            .cursor_pointer()
            .on_click(move |_event, window, cx| {
                let _ = weak.update(cx, |view, vcx| match kind {
                    PlaceholderKind::Field => view.begin_empty_field(window, vcx),
                    PlaceholderKind::Index => view.begin_empty_index(window, vcx),
                    PlaceholderKind::ForeignKey => view.begin_empty_foreign_key(window, vcx),
                    PlaceholderKind::UniqueKey => view.begin_empty_unique_key(window, vcx),
                    PlaceholderKind::Check => view.begin_empty_check(window, vcx),
                });
            }),
        );
    }
    row
}

fn design_cell(id: SharedString, width: f32, theme: Theme, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(DESIGN_ROW_HEIGHT))
        .w(px(width))
        .flex_none()
        .whitespace_nowrap()
        .overflow_hidden()
        .border_r_1()
        .border_b_1()
        .border_color(rgb(theme.grid_line))
        .when(selected, |style| {
            style.text_color(rgb(theme.grid_selection_text))
        })
}

#[allow(clippy::too_many_arguments)]
fn design_edit_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    column: FieldColumn,
    editor: Option<Entity<TextInput>>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, selected)
        .when(selected, |cell| cell.bg(rgb(theme.grid_selection_bg)));
    if let Some(editor) = editor {
        return cell
            .bg(rgb(theme.input_bg))
            // The selected-cell text color (white) must not leak into the editor, or its text is
            // invisible on the light input background.
            .text_color(rgb(theme.text))
            .child(div().w(px(width)).h(px(DESIGN_ROW_HEIGHT)).child(editor))
            .into_any_element();
    }
    let weak = weak.clone();
    cell.pr(px(4.0))
        .cursor_text()
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| view.begin_edit(row, column, window, cx));
        })
        .child(text)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn index_edit_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    column: IndexColumn,
    editor: Option<Entity<TextInput>>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, selected)
        .when(selected, |cell| cell.bg(rgb(theme.grid_selection_bg)));
    if let Some(editor) = editor {
        return cell
            .bg(rgb(theme.input_bg))
            // The selected-cell text color (white) must not leak into the editor, or its text is
            // invisible on the light input background.
            .text_color(rgb(theme.text))
            .child(div().w(px(width)).h(px(DESIGN_ROW_HEIGHT)).child(editor))
            .into_any_element();
    }
    let weak = weak.clone();
    cell.pr(px(4.0))
        .cursor_text()
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.begin_index_edit(row, column, window, cx)
            });
        })
        .child(text)
        .into_any_element()
}

/// A dropdown-style cell of the index grid (kind or method).
#[allow(clippy::too_many_arguments)]
fn index_choice_cell(
    id: SharedString,
    width: f32,
    text: String,
    enabled: bool,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    field: IndexComboField,
    anchor: IndexComboAnchors,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .flex()
        .items_center()
        .h(px(DESIGN_ROW_HEIGHT))
        .w(px(width))
        .flex_none()
        .whitespace_nowrap()
        .overflow_hidden()
        .border_r_1()
        .border_b_1()
        .border_color(rgb(theme.grid_line))
        .when(selected, |style| {
            style.text_color(rgb(theme.grid_selection_text))
        })
        .pr(px(4.0))
        .when(enabled, |style| style.cursor_pointer())
        .on_children_prepainted(move |bounds, _window, _cx| {
            if let Some(first) = bounds.first() {
                anchor
                    .borrow_mut()
                    .insert((row, field), Point::new(first.left(), first.bottom()));
            }
        })
        .id(id)
        .on_click(move |_event, window, cx| {
            if enabled {
                let _ = weak.update(cx, |view, cx| view.open_index_combo(row, field, window, cx));
            }
        })
        .child(div().flex_1().min_w(px(0.0)).overflow_hidden().child(text))
        .child(
            svg()
                .path("icons/chevron-down.svg")
                .w(px(10.0))
                .h(px(10.0))
                .flex_none()
                .text_color(rgb(theme.text_muted))
                .opacity(if enabled { 1.0 } else { 0.0 }),
        )
        .into_any_element()
}

/// One row of an index kind/method dropdown popup.
#[allow(clippy::too_many_arguments)]
fn index_choice_option(
    id: String,
    label: String,
    selected: bool,
    row: usize,
    field: IndexComboField,
    value: &'static str,
    weak: &WeakEntity<TableDesignView>,
    theme: Theme,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .id(SharedString::from(id))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_1()
        .h(px(20.0))
        .px_2()
        .flex_none()
        .text_size(px(12.0))
        .cursor_pointer()
        .when(selected, move |style| {
            style
                .bg(rgb(theme.tree_selected_bg))
                .text_color(rgb(theme.tree_selected_text))
        })
        .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
        .on_click(move |_event, _window, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.select_index_combo(row, field, value, cx)
            });
        })
        .child(label)
        .child(
            svg()
                .path("icons/check.svg")
                .w(px(12.0))
                .h(px(12.0))
                .flex_none()
                .text_color(rgb(theme.tree_selected_text))
                .opacity(if selected { 1.0 } else { 0.0 }),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn design_type_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    anchor: Rc<RefCell<BTreeMap<usize, Point<Pixels>>>>,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .flex()
        .items_center()
        .h(px(DESIGN_ROW_HEIGHT))
        .w(px(width))
        .flex_none()
        .whitespace_nowrap()
        .overflow_hidden()
        .border_r_1()
        .border_b_1()
        .border_color(rgb(theme.grid_line))
        .when(selected, |style| {
            style.text_color(rgb(theme.grid_selection_text))
        })
        .pr(px(4.0))
        .cursor_pointer()
        .on_children_prepainted(move |bounds, _window, _cx| {
            if let Some(first) = bounds.first() {
                anchor
                    .borrow_mut()
                    .insert(row, Point::new(first.left(), first.bottom()));
            }
        })
        .id(id)
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| view.open_type_combo(row, window, cx));
        })
        .child(div().flex_1().min_w(px(0.0)).overflow_hidden().child(text))
        .child(
            svg()
                .path("icons/chevron-down.svg")
                .w(px(10.0))
                .h(px(10.0))
                .flex_none()
                .text_color(rgb(theme.text_muted)),
        )
        .into_any_element()
}

fn design_check_cell(
    id: SharedString,
    width: f32,
    checked: bool,
    theme: Theme,
    toggle: Option<(WeakEntity<TableDesignView>, usize)>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, false)
        .px_2()
        .child(checkbox_box(checked, theme));
    let Some((weak, row)) = toggle else {
        return cell.into_any_element();
    };
    cell.cursor_pointer()
        .on_click(move |_event, _window, cx| {
            let _ = weak.update(cx, |view, cx| view.toggle_not_null(row, cx));
        })
        .into_any_element()
}

fn design_key_cell(
    id: SharedString,
    width: f32,
    primary_key: bool,
    theme: Theme,
    toggle: Option<(WeakEntity<TableDesignView>, usize)>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, false)
        .px_2()
        .child(checkbox_box(primary_key, theme));
    let Some((weak, row)) = toggle else {
        return cell.into_any_element();
    };
    cell.cursor_pointer()
        .on_click(move |_event, _window, cx| {
            let _ = weak.update(cx, |view, cx| view.toggle_field_primary_key(row, cx));
        })
        .into_any_element()
}

/// One row's cell for an engine-declared field-list column. The cell is a text editor, a toggle or
/// a read-only display depending on the column kind, so MySQL's 长度/小数位/虚拟 and SQL Server's
/// 大小/比例/计算列/排序规则/稀疏 all render correctly.
#[allow(clippy::too_many_arguments)]
fn design_field_column_cell(
    kind: DesignerFieldColumn,
    row: usize,
    column: &ColumnDef,
    width: f32,
    theme: Theme,
    weak: &WeakEntity<TableDesignView>,
    editor: Option<Entity<TextInput>>,
) -> AnyElement {
    let id = SharedString::from(format!("design-{}-{row}", field_column_id(kind)));
    if let Some(text_column) = field_column_text(kind) {
        return design_edit_cell(
            id,
            width,
            field_column_value(column, text_column),
            theme,
            false,
            weak,
            row,
            text_column,
            editor,
        );
    }
    match kind {
        DesignerFieldColumn::NotNull => design_check_cell(
            id,
            width,
            !column.nullable,
            theme,
            Some((weak.clone(), row)),
        ),
        DesignerFieldColumn::Generated | DesignerFieldColumn::Computed => {
            design_check_cell(id, width, !column.generated.trim().is_empty(), theme, None)
        }
        DesignerFieldColumn::Sparse => design_check_cell(id, width, column.sparse, theme, None),
        DesignerFieldColumn::Key => design_key_cell(
            id,
            width,
            column.primary_key,
            theme,
            Some((weak.clone(), row)),
        ),
        _ => design_cell(id, width, theme, false)
            .px_2()
            .child(field_column_display(kind, column))
            .into_any_element(),
    }
}

/// A designer grid's header row: a gutter spacer plus one labelled cell per column.
fn design_grid_header(theme: Theme, labels: &[&'static str], widths: &[f32]) -> Div {
    let mut header = div()
        .flex()
        .flex_row()
        .flex_none()
        .h(px(DESIGN_HEADER_HEIGHT))
        .w_full()
        .bg(rgb(theme.header_bg))
        .border_b_1()
        .border_color(rgb(theme.border))
        .child(
            div()
                .w(px(DESIGN_GUTTER_WIDTH))
                .h(px(DESIGN_HEADER_HEIGHT))
                .flex_none()
                .border_r_1()
                .border_color(rgb(theme.grid_line)),
        );
    for (index, key) in labels.iter().enumerate() {
        header = header.child(
            div()
                .flex()
                .items_center()
                .h(px(DESIGN_HEADER_HEIGHT))
                .w(px(widths.get(index).copied().unwrap_or(120.0)))
                .flex_none()
                .px_2()
                .whitespace_nowrap()
                .overflow_hidden()
                .border_r_1()
                .border_color(rgb(theme.grid_line))
                .child(t!(*key).to_string()),
        );
    }
    header
}

/// Which field drop-down a picker cell opens. Every designer grid that is made of a *list* of
/// columns (an index, a unique key, a foreign key's child and referenced columns) uses one of these.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DesignPicker {
    /// The index grid's field list.
    IndexFields,
    /// A foreign key's own (child) columns.
    ForeignKeyColumns,
    /// A foreign key's referenced columns.
    ForeignKeyReferenced,
    /// The unique-key grid's field list.
    UniqueKeyFields,
}

/// A dropdown-style cell holding a list of columns: the whole cell is the trigger (no separate
/// "..." button), and the list it opens keeps its check boxes so a multi-column constraint stays
/// selectable. `enabled` is `false` for a cell the designer does not edit (the primary key's field
/// list, which is driven by the 字段 tab's 主键 box) — it keeps the space but shows no chevron.
#[allow(clippy::too_many_arguments)]
fn design_picker_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    enabled: bool,
    picker: DesignPicker,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    anchor: DesignPickerAnchors,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .flex()
        .items_center()
        .h(px(DESIGN_ROW_HEIGHT))
        .w(px(width))
        .flex_none()
        .whitespace_nowrap()
        .overflow_hidden()
        .border_r_1()
        .border_b_1()
        .border_color(rgb(theme.grid_line))
        .pr(px(4.0))
        .when(enabled, |style| style.cursor_pointer())
        .on_children_prepainted(move |bounds, _window, _cx| {
            if let Some(first) = bounds.first() {
                anchor
                    .borrow_mut()
                    .insert(row, Point::new(first.left(), first.bottom()));
            }
        })
        .id(id)
        .on_click(move |_event, window, cx| {
            if enabled {
                let _ = weak.update(cx, |view, cx| {
                    view.open_design_picker(row, picker, window, cx)
                });
            }
        })
        // The text fills the row's height so the recorded anchor is the cell's own bottom edge — the
        // drop-down list then opens flush under the cell, as a drop-down should.
        .child(
            div()
                .flex()
                .items_center()
                .flex_1()
                .min_w(px(0.0))
                .h(px(DESIGN_ROW_HEIGHT))
                .overflow_hidden()
                .child(text),
        )
        .child(
            svg()
                .path("icons/chevron-down.svg")
                .w(px(10.0))
                .h(px(10.0))
                .flex_none()
                .text_color(rgb(theme.text_muted))
                .opacity(if enabled { 1.0 } else { 0.0 }),
        )
        .into_any_element()
}

/// A live-editable text cell of the unique-key grid.
#[allow(clippy::too_many_arguments)]
fn unique_key_edit_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    editor: Option<Entity<TextInput>>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, selected)
        .when(selected, |cell| cell.bg(rgb(theme.grid_selection_bg)));
    if let Some(editor) = editor {
        return cell
            .bg(rgb(theme.input_bg))
            // The selected-cell text color (white) must not leak into the editor, or its text is
            // invisible on the light input background.
            .text_color(rgb(theme.text))
            .child(div().w(px(width)).h(px(DESIGN_ROW_HEIGHT)).child(editor))
            .into_any_element();
    }
    let weak = weak.clone();
    cell.pr(px(4.0))
        .cursor_text()
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.begin_unique_key_edit(row, UniqueKeyColumn::Name, window, cx)
            });
        })
        .child(text)
        .into_any_element()
}

/// A live-editable text cell of the check-constraint grid.
#[allow(clippy::too_many_arguments)]
fn check_edit_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    column: CheckColumn,
    editor: Option<Entity<TextInput>>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, selected)
        .when(selected, |cell| cell.bg(rgb(theme.grid_selection_bg)));
    if let Some(editor) = editor {
        return cell
            .bg(rgb(theme.input_bg))
            // The selected-cell text color (white) must not leak into the editor, or its text is
            // invisible on the light input background.
            .text_color(rgb(theme.text))
            .child(div().w(px(width)).h(px(DESIGN_ROW_HEIGHT)).child(editor))
            .into_any_element();
    }
    let weak = weak.clone();
    cell.pr(px(4.0))
        .cursor_text()
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.begin_check_edit(row, column, window, cx)
            });
        })
        .child(text)
        .into_any_element()
}

/// One labelled row of the detail panel whose control is a field (text input or drop-down).
fn detail_editor_row(label: impl Into<String>, control: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(230.0))
                .flex_none()
                .text_size(px(12.0))
                .child(label.into()),
        )
        .child(control)
        .into_any_element()
}

/// One check-box row of the detail panel. The panel itself is muted while no field is selected.
fn detail_toggle_row(
    option: DesignerColumnOption,
    checked: bool,
    enabled: bool,
    theme: Theme,
    weak: WeakEntity<TableDesignView>,
) -> AnyElement {
    div()
        .id(SharedString::from(format!(
            "design-detail-{}",
            detail_option_id(option)
        )))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .when(!enabled, |style| style.opacity(0.5))
        .when(enabled, |style| style.cursor_pointer())
        .child(checkbox_box(checked, theme))
        .child(
            div()
                .text_size(px(12.0))
                .child(t!(option.label_key()).to_string()),
        )
        .when(enabled, |style| {
            style.on_click(move |_event, _window, cx| {
                let _ = weak.update(cx, |view, cx| view.toggle_detail(option, cx));
            })
        })
        .into_any_element()
}

/// A stable id fragment for one detail-panel control.
fn detail_option_id(option: DesignerColumnOption) -> &'static str {
    match option {
        DesignerColumnOption::Default => "default",
        DesignerColumnOption::AutoIncrement => "auto-increment",
        DesignerColumnOption::Unsigned => "unsigned",
        DesignerColumnOption::Zerofill => "zerofill",
        DesignerColumnOption::Collation => "collation",
        DesignerColumnOption::Sparse => "sparse",
    }
}

fn rule_label(rule: &str) -> String {
    if rule.trim().is_empty() {
        t!("design.fk.rule_default").to_string()
    } else {
        rule.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn fk_edit_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    column: FkColumn,
    editor: Option<Entity<TextInput>>,
) -> AnyElement {
    let cell = design_cell(id, width, theme, selected)
        .when(selected, |cell| cell.bg(rgb(theme.grid_selection_bg)));
    if let Some(editor) = editor {
        return cell
            .bg(rgb(theme.input_bg))
            // The selected-cell text color (white) must not leak into the editor, or its text is
            // invisible on the light input background.
            .text_color(rgb(theme.text))
            .child(div().w(px(width)).h(px(DESIGN_ROW_HEIGHT)).child(editor))
            .into_any_element();
    }
    let weak = weak.clone();
    cell.pr(px(4.0))
        .cursor_text()
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| view.begin_fk_edit(row, column, window, cx));
        })
        .child(text)
        .into_any_element()
}

/// A dropdown-style cell of the foreign-key grid (on delete / on update).
#[allow(clippy::too_many_arguments)]
fn fk_choice_cell(
    id: SharedString,
    width: f32,
    text: String,
    theme: Theme,
    selected: bool,
    weak: &WeakEntity<TableDesignView>,
    row: usize,
    field: FkComboField,
    anchor: FkComboAnchors,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .flex()
        .items_center()
        .h(px(DESIGN_ROW_HEIGHT))
        .w(px(width))
        .flex_none()
        .whitespace_nowrap()
        .overflow_hidden()
        .border_r_1()
        .border_b_1()
        .border_color(rgb(theme.grid_line))
        .when(selected, |style| {
            style.text_color(rgb(theme.grid_selection_text))
        })
        .pr(px(4.0))
        .cursor_pointer()
        .on_children_prepainted(move |bounds, _window, _cx| {
            if let Some(first) = bounds.first() {
                anchor
                    .borrow_mut()
                    .insert((row, field), Point::new(first.left(), first.bottom()));
            }
        })
        .id(id)
        .on_click(move |_event, window, cx| {
            let _ = weak.update(cx, |view, cx| view.open_fk_combo(row, field, window, cx));
        })
        .child(div().flex_1().min_w(px(0.0)).overflow_hidden().child(text))
        .child(
            svg()
                .path("icons/chevron-down.svg")
                .w(px(10.0))
                .h(px(10.0))
                .flex_none()
                .text_color(rgb(theme.text_muted)),
        )
        .into_any_element()
}

/// One row of a foreign-key rule dropdown popup.
#[allow(clippy::too_many_arguments)]
fn fk_choice_option(
    id: String,
    label: String,
    selected: bool,
    row: usize,
    field: FkComboField,
    value: &'static str,
    weak: &WeakEntity<TableDesignView>,
    theme: Theme,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .id(SharedString::from(id))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_1()
        .h(px(20.0))
        .px_2()
        .flex_none()
        .text_size(px(12.0))
        .cursor_pointer()
        .when(selected, move |style| {
            style
                .bg(rgb(theme.tree_selected_bg))
                .text_color(rgb(theme.tree_selected_text))
        })
        .hover(move |style| style.bg(rgb(theme.tree_hover_bg)))
        .on_click(move |_event, _window, cx| {
            let _ = weak.update(cx, |view, cx| view.select_fk_combo(row, field, value, cx));
        })
        .child(label)
        .child(
            svg()
                .path("icons/check.svg")
                .w(px(12.0))
                .h(px(12.0))
                .flex_none()
                .text_color(rgb(theme.tree_selected_text))
                .opacity(if selected { 1.0 } else { 0.0 }),
        )
        .into_any_element()
}

fn option_row(label: impl Into<String>, input: Entity<TextInput>, theme: Theme) -> Div {
    let label = label.into();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(150.0))
                .flex_none()
                .text_size(px(12.0))
                .child(label),
        )
        .child(div().w(px(300.0)).h(px(22.0)).child(input))
        .text_color(rgb(theme.text))
}

/// A labelled drop-down row (`label: [combo]`) for the Options tab.
fn option_combo_row(
    label: impl Into<String>,
    combo: Option<Entity<ComboBox>>,
    theme: Theme,
) -> Div {
    let label = label.into();
    let mut row = div().flex().flex_row().items_center().gap_2().child(
        div()
            .w(px(150.0))
            .flex_none()
            .text_size(px(12.0))
            .child(label),
    );
    if let Some(combo) = combo {
        row = row.child(combo);
    }
    row.text_color(rgb(theme.text))
}
