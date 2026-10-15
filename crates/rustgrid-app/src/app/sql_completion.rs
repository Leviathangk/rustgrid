//! The SQL completion provider for gpui-kit's `Editor`.
//!
//! The provider is a thin adapter: it snapshots the caret's statement with `sql::completion_context`
//! (the same analysis the hand-rolled popup used) and turns the resulting candidates into LSP
//! `CompletionItem`s. It reads the connection catalog (tables, columns, functions) from a
//! [`CompletionSource`] the app keeps refreshed.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, Task, Window};
use gpui_kit::component::input::CompletionProvider;
use gpui_kit::component::input::RopeExt as _;
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Range, TextEdit,
};
use ropey::Rope;
use rustgrid_core::DriverDialect;

use crate::sql;

/// One column in the catalog: its name, type and comment.
pub(crate) type CatalogColumn = (String, String, String);

/// The catalog completion draws on, keyed by connection index. `AppView` replaces this snapshot
/// whenever the loaded catalogs change.
#[derive(Default)]
pub(crate) struct CompletionCatalog {
    /// `(connection index, database)` → table names.
    pub tables: HashMap<(usize, String), Vec<String>>,
    /// `(connection index, database, table)` → columns.
    pub columns: HashMap<(usize, String, String), Vec<CatalogColumn>>,
    /// Connection index → stored function names.
    pub functions: HashMap<usize, Vec<String>>,
}

/// The connection / database / schema the active editor's completion and column lookups target.
#[derive(Clone, Default)]
pub(crate) struct CompletionScope {
    pub connection_index: Option<usize>,
    pub database: Option<String>,
    /// The selected schema (SQL Server); `None` leaves unqualified objects to the engine default.
    pub schema: Option<String>,
    /// Whether the target engine namespaces objects by schema (SQL Server). When true a referenced
    /// table's first qualifier is a schema, not a database.
    pub supports_schemas: bool,
    /// The connection's SQL dialect, used to pick the built-in function list so an engine is not
    /// offered another's functions.
    pub dialect: DriverDialect,
}

/// A shared catalog and the scope the active editor targets. The scope is behind an `Arc` so a
/// live provider can be re-scoped when the toolbar's connection/database/schema changes.
#[derive(Clone)]
pub(crate) struct CompletionSource {
    catalog: Arc<std::sync::RwLock<CompletionCatalog>>,
    scope: Arc<std::sync::RwLock<CompletionScope>>,
}

impl CompletionSource {
    pub(crate) fn new(catalog: Arc<std::sync::RwLock<CompletionCatalog>>) -> Self {
        Self {
            catalog,
            scope: Arc::new(std::sync::RwLock::new(CompletionScope::default())),
        }
    }

    pub(crate) fn with_scope(
        self,
        connection_index: Option<usize>,
        database: Option<String>,
        schema: Option<String>,
        supports_schemas: bool,
        dialect: DriverDialect,
    ) -> Self {
        *self.scope.write().expect("completion scope lock") = CompletionScope {
            connection_index,
            database,
            schema,
            supports_schemas,
            dialect,
        };
        self
    }

    /// The shared scope, so the editor wrapper can re-scope a live provider.
    pub(crate) fn scope_handle(&self) -> Arc<std::sync::RwLock<CompletionScope>> {
        self.scope.clone()
    }
}

/// Resolve a referenced table to the catalog's `(database, table-key)` coordinates. On schema
/// engines the reference's first qualifier is a schema (not a database), and the catalog keys
/// tables as `schema.table`; elsewhere the qualifier is a database and the key is the bare name.
pub(crate) fn catalog_target(
    scope: &CompletionScope,
    table: &sql::SqlTableRef,
) -> Option<(String, String)> {
    let qualifier = table.database.as_deref().filter(|name| !name.is_empty());
    if scope.supports_schemas {
        let database = scope.database.clone()?;
        let schema = qualifier
            .map(str::to_string)
            .or_else(|| scope.schema.clone());
        let key = match schema {
            Some(schema) => format!("{schema}.{}", table.name),
            None => table.name.clone(),
        };
        Some((database, key))
    } else {
        let database = qualifier
            .map(str::to_string)
            .or_else(|| scope.database.clone())?;
        Some((database, table.name.clone()))
    }
}

/// The gpui-kit completion provider over a [`CompletionSource`].
pub(crate) struct SqlCompletionProvider {
    source: CompletionSource,
}

impl SqlCompletionProvider {
    pub(crate) fn new(source: CompletionSource) -> Self {
        Self { source }
    }
}

impl CompletionProvider for SqlCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let sql_text = text.to_string();
        let dialect = self
            .source
            .scope
            .read()
            .expect("completion scope lock")
            .dialect;
        let context = sql::completion_context(&sql_text, offset, dialect);
        let mut items = build_items(&self.source, &context);
        // Anchor the replacement range explicitly. gpui's completion menu derives it from the
        // offset of the *first* keystroke that opened the menu (`trigger_start_offset`), so
        // accepting "SELECT" after typing "sele" inserts at the wrong place ("seSELECT"). An
        // explicit `text_edit` overrides that range with the real identifier fragment
        // (`prefix_start..offset`).
        let range = replacement_range(text, &context, offset);
        for item in &mut items {
            item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: item.label.clone(),
            }));
        }
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        // Open the menu on an identifier character or a `.` qualifier; the provider then filters by
        // the typed prefix.
        new_text
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == '.')
    }
}

/// The completion groups, in the order the popup lists them: the fields of the statement at hand,
/// then the language's everyday words, then the user's own tables — Navicat's order, where the most
/// used keyword leads the table names — then the rest of the language.
const GROUP_COLUMNS: u8 = 0;
const GROUP_COMMON_KEYWORDS: u8 = 1;
const GROUP_TABLES: u8 = 2;
const GROUP_KEYWORDS: u8 = 3;
const GROUP_FUNCTIONS: u8 = 4;

/// Whether a group's candidates keep the order they were built in instead of being re-sorted by
/// length and then alphabetically: the table's own field order for columns, and the curated usage
/// order of [`sql::COMMON_KEYWORDS`] for the everyday keywords.
fn keeps_build_order(group: u8) -> bool {
    matches!(group, GROUP_COLUMNS | GROUP_COMMON_KEYWORDS)
}

/// Build the completion items for a caret context, ordering columns → everyday keywords → tables →
/// other keywords → functions and de-duplicating by `(label, detail)`.
///
/// A candidate matches by what the user typed anywhere in its name — exactly, as a prefix, inside
/// it or as a loose subsequence ([`match_quality`]) — so `as` finds the table `wms_asn` and `wsn`
/// still does. The closest matches lead; at equal quality the order above holds, which is Navicat's:
/// the everyday keyword the caret is most likely asking for (`SELECT` for `se`) leads the user's
/// table names, unless a field of the statement's own tables matches.
///
/// Under a selected schema a table/function is offered by its bare name, so the typed fragment
/// matches and accepting it does not append the schema.
fn build_items(
    source: &CompletionSource,
    context: &sql::SqlCompletionContext,
) -> Vec<CompletionItem> {
    if context.suppress {
        return Vec::new();
    }
    let catalog = source.catalog.read().expect("completion catalog lock");
    let scope = source.scope.read().expect("completion scope lock");
    let connection = scope.connection_index;
    let qualifier = context.qualifier.last().map(String::as_str);

    let kind = if context.kind == sql::SqlCompletionKind::Table {
        CompletionItemKind::CLASS
    } else {
        CompletionItemKind::FIELD
    };

    let mut items: Vec<(u8, CompletionItem)> = Vec::new();

    // 1. Columns: from the statement's referenced tables, or the qualifier's table.
    let column_targets: Vec<(String, String)> = if let Some(qualifier) = qualifier {
        resolve_qualified_table(context, qualifier)
            .and_then(|table| catalog_target(&scope, table))
            .into_iter()
            .collect()
    } else {
        context
            .tables
            .iter()
            .filter_map(|table| catalog_target(&scope, table))
            .collect()
    };
    if let Some(connection) = connection {
        for (database, table) in &column_targets {
            if let Some(columns) =
                catalog
                    .columns
                    .get(&(connection, database.clone(), table.clone()))
            {
                for (name, data_type, comment) in columns {
                    let detail = if comment.is_empty() {
                        format!("{table} [{data_type}]")
                    } else {
                        format!("{table} [{data_type}]  {comment}")
                    };
                    items.push((GROUP_COLUMNS, completion_item(name, &detail, kind)));
                }
            }
        }
    }

    // 2. Tables: all loaded tables, or the qualifier database's tables. With a schema selected,
    // only that schema's tables are offered (the schema selector's filtering).
    let table_kind = CompletionItemKind::CLASS;
    if let Some(connection) = connection {
        let mut tables: Vec<(String, String)> = Vec::new();
        for ((conn, database), names) in &catalog.tables {
            if *conn != connection {
                continue;
            }
            if let Some(qualifier) = qualifier
                && !database.eq_ignore_ascii_case(qualifier)
            {
                continue;
            }
            for name in names {
                if qualifier.is_none()
                    && let Some(schema) = &scope.schema
                    && !name_in_schema(name, schema)
                {
                    continue;
                }
                tables.push((name.clone(), database.clone()));
            }
        }
        tables.sort();
        tables.dedup();
        for (name, database) in tables {
            let label = display_name(&name, scope.schema.as_deref());
            items.push((GROUP_TABLES, completion_item(&label, &database, table_kind)));
        }
    }

    // 3. Keywords and 4. functions: built-ins plus the connection's stored functions. The everyday
    // keywords are offered first, in their curated (most-typed first) order, so a shared prefix
    // leads with the word in every statement; the rest of the language's keywords follow, and
    // functions last.
    if qualifier.is_none() {
        for keyword in sql::COMMON_KEYWORDS {
            items.push((
                GROUP_COMMON_KEYWORDS,
                completion_item(keyword, "keyword", CompletionItemKind::KEYWORD),
            ));
        }
        for keyword in sql::keywords() {
            if sql::is_common_keyword(keyword) {
                continue;
            }
            items.push((
                GROUP_KEYWORDS,
                completion_item(keyword, "keyword", CompletionItemKind::KEYWORD),
            ));
        }
        for function in sql::functions_for(scope.dialect).iter().copied() {
            items.push((
                GROUP_FUNCTIONS,
                completion_item(function, "function", CompletionItemKind::FUNCTION),
            ));
        }
        if let Some(connection) = connection
            && let Some(functions) = catalog.functions.get(&connection)
        {
            for function in functions {
                if let Some(schema) = &scope.schema
                    && !name_in_schema(function, schema)
                {
                    continue;
                }
                let label = display_name(function, scope.schema.as_deref());
                items.push((
                    GROUP_FUNCTIONS,
                    completion_item(&label, "function", CompletionItemKind::FUNCTION),
                ));
            }
        }
    }

    let prefix = context.prefix.as_str();
    let mut seen = std::collections::HashSet::new();
    let mut filtered: Vec<(Match, u8, CompletionItem)> = items
        .into_iter()
        .filter_map(|(group, item)| Some((match_quality(&item.label, prefix)?, group, item)))
        .filter(|(_, _, item)| {
            seen.insert((
                item.label.to_lowercase(),
                item.detail.clone().unwrap_or_default(),
            ))
        })
        .collect();
    filtered.sort_by(|a, b| {
        // The closest match leads, and at equal quality the group above decides — the everyday
        // keyword before the table names (Navicat's order).
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            // Within a group the candidate closest to what was typed leads: `fr` offers `FREE`
            // before `FREEZE` before `FRAME_ROW`. Columns keep the table's own field order and the
            // everyday keywords their curated usage order, so those two groups are left as built.
            .then_with(|| {
                if keeps_build_order(a.1) {
                    std::cmp::Ordering::Equal
                } else {
                    a.2.label.chars().count().cmp(&b.2.label.chars().count())
                }
            })
            .then_with(|| {
                if keeps_build_order(a.1) {
                    std::cmp::Ordering::Equal
                } else {
                    a.2.label.to_lowercase().cmp(&b.2.label.to_lowercase())
                }
            })
    });
    filtered.truncate(64);
    filtered.into_iter().map(|(_, _, item)| item).collect()
}

/// How a candidate's label matched the typed fragment, best first: the popup lists the closest
/// matches above the loosest ones.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    /// The label is exactly what was typed.
    Exact,
    /// The label starts with what was typed (`users` for `use`).
    Prefix,
    /// The typed fragment appears inside the label (`wms_asn` for `as`).
    Substring,
    /// The typed characters appear in order but apart (`wms_asn` for `wsn`).
    Subsequence,
}

/// How `label` matches the already-lowercased `fragment`, or `None` when it does not match at all.
/// Table and column names rarely start with what the user remembers about them (`wms_asn` for
/// `asn`), so anything but the first fragment character may also appear inside the name.
fn match_quality(label: &str, fragment: &str) -> Option<Match> {
    let label = label.to_lowercase();
    if fragment.is_empty() {
        return Some(Match::Prefix);
    }
    if label == fragment {
        return Some(Match::Exact);
    }
    if label.starts_with(fragment) {
        return Some(Match::Prefix);
    }
    if label.contains(fragment) {
        return Some(Match::Substring);
    }
    let mut label = label.chars();
    fragment
        .chars()
        .all(|wanted| label.by_ref().any(|found| found == wanted))
        .then_some(Match::Subsequence)
}

/// The completion label for a possibly schema-qualified object name. Under a selected schema the
/// bare name is shown (and inserted), so accepting a suggestion does not append the schema.
fn display_name(name: &str, schema: Option<&str>) -> String {
    if let Some(schema) = schema
        && let Some((prefix, object)) = name.split_once('.')
        && !prefix.is_empty()
        && !object.is_empty()
        && prefix.eq_ignore_ascii_case(schema)
    {
        return object.to_string();
    }
    name.to_string()
}

/// Resolve a single-part qualifier (`alias` or `table` or `database`) to its referenced table.
fn resolve_qualified_table<'a>(
    context: &'a sql::SqlCompletionContext,
    qualifier: &str,
) -> Option<&'a sql::SqlTableRef> {
    context.tables.iter().find(|table| {
        table
            .alias
            .as_deref()
            .is_some_and(|alias| alias.eq_ignore_ascii_case(qualifier))
            || table.name.eq_ignore_ascii_case(qualifier)
    })
}

/// Whether a (possibly schema-qualified) catalog object name belongs to `schema`.
fn name_in_schema(name: &str, schema: &str) -> bool {
    name.split_once('.').is_some_and(|(prefix, object)| {
        !prefix.is_empty() && !object.is_empty() && prefix.eq_ignore_ascii_case(schema)
    })
}

fn completion_item(label: &str, detail: &str, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        label_details: None,
        kind: Some(kind),
        detail: Some(detail.to_string()),
        ..Default::default()
    }
}

/// The range (in gpui's char-column positions) that accepting a completion must replace: the
/// identifier fragment from `context.prefix_start` up to the caret offset.
fn replacement_range(text: &Rope, context: &sql::SqlCompletionContext, offset: usize) -> Range {
    Range::new(
        text.offset_to_position(context.prefix_start),
        text.offset_to_position(offset),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_range_covers_the_typed_identifier() {
        let sql = "SELECT * FROM ams";
        let context = crate::sql::completion_context(sql, sql.len(), DriverDialect::Mysql);
        let range = replacement_range(&Rope::from(sql), &context, sql.len());
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, "SELECT * FROM ".len() as u32);
        assert_eq!(range.end.character, sql.len() as u32);
    }

    #[test]
    fn replacement_range_is_empty_at_a_fresh_dot() {
        let sql = "SELECT t.";
        let context = crate::sql::completion_context(sql, sql.len(), DriverDialect::Mysql);
        let range = replacement_range(&Rope::from(sql), &context, sql.len());
        assert_eq!(range.start, range.end);
        assert_eq!(range.end.character, sql.len() as u32);
    }

    fn schema_scope(schema: Option<&str>) -> CompletionScope {
        CompletionScope {
            connection_index: Some(0),
            database: Some("appdb".into()),
            schema: schema.map(str::to_string),
            supports_schemas: true,
            dialect: DriverDialect::Generic,
        }
    }

    fn table(database: Option<&str>, name: &str) -> sql::SqlTableRef {
        sql::SqlTableRef {
            database: database.map(str::to_string),
            name: name.to_string(),
            alias: None,
        }
    }

    #[test]
    fn catalog_target_qualifies_unqualified_tables_on_schema_engines() {
        let scope = schema_scope(Some("sales"));
        assert_eq!(
            catalog_target(&scope, &table(None, "orders")),
            Some(("appdb".into(), "sales.orders".into()))
        );
    }

    #[test]
    fn catalog_target_prefers_the_reference_schema_over_the_selected_one() {
        let scope = schema_scope(Some("sales"));
        assert_eq!(
            catalog_target(&scope, &table(Some("dbo"), "users")),
            Some(("appdb".into(), "dbo.users".into()))
        );
    }

    #[test]
    fn catalog_target_leaves_unqualified_tables_bare_without_a_schema() {
        let scope = schema_scope(None);
        assert_eq!(
            catalog_target(&scope, &table(None, "orders")),
            Some(("appdb".into(), "orders".into()))
        );
    }

    #[test]
    fn catalog_target_keeps_database_qualifiers_on_schema_less_engines() {
        let scope = CompletionScope {
            connection_index: Some(0),
            database: Some("default".into()),
            schema: None,
            supports_schemas: false,
            dialect: DriverDialect::Generic,
        };
        assert_eq!(
            catalog_target(&scope, &table(None, "orders")),
            Some(("default".into(), "orders".into()))
        );
        assert_eq!(
            catalog_target(&scope, &table(Some("other"), "orders")),
            Some(("other".into(), "orders".into()))
        );
    }

    #[test]
    fn name_in_schema_matches_the_prefix() {
        assert!(name_in_schema("dbo.users", "DBO"));
        assert!(!name_in_schema("sales.users", "dbo"));
        assert!(!name_in_schema("users", "dbo"));
    }

    #[test]
    fn display_name_strips_the_selected_schema_prefix() {
        assert_eq!(display_name("dbo.users", Some("DBO")), "users");
        // A name outside the selected schema keeps its qualified label.
        assert_eq!(display_name("sales.users", Some("dbo")), "sales.users");
        // Without a selected schema (or for an unqualified name) the name is unchanged.
        assert_eq!(display_name("dbo.users", None), "dbo.users");
        assert_eq!(display_name("users", Some("dbo")), "users");
    }

    /// A source with an empty catalog: only the built-in keywords and functions are offered, which
    /// is all the ordering test needs.
    fn empty_catalog_source() -> CompletionSource {
        CompletionSource::new(Arc::new(std::sync::RwLock::new(
            CompletionCatalog::default(),
        )))
        .with_scope(
            Some(0),
            Some("appdb".into()),
            None,
            false,
            DriverDialect::Mysql,
        )
    }

    /// A source whose catalog holds `tables`, and `columns` of `table`, in `appdb`.
    fn catalog_source(tables: &[&str], columns: &[(&str, &[&str])]) -> CompletionSource {
        let mut catalog = CompletionCatalog::default();
        catalog.tables.insert(
            (0, "appdb".to_string()),
            tables.iter().map(|table| table.to_string()).collect(),
        );
        for (table, names) in columns {
            catalog.columns.insert(
                (0, "appdb".to_string(), table.to_string()),
                names
                    .iter()
                    .map(|name| (name.to_string(), "int".to_string(), String::new()))
                    .collect(),
            );
        }
        CompletionSource::new(Arc::new(std::sync::RwLock::new(catalog))).with_scope(
            Some(0),
            Some("appdb".into()),
            None,
            false,
            DriverDialect::Mysql,
        )
    }

    fn labels_for(source: &CompletionSource, sql: &str) -> Vec<String> {
        let context = crate::sql::completion_context(sql, sql.len(), DriverDialect::Mysql);
        build_items(source, &context)
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    #[test]
    fn a_table_is_found_by_any_part_of_its_name() {
        let source = catalog_source(&["wms_asn", "users"], &[]);
        // `as` is inside `wms_asn`, so the table is offered rather than filtered away...
        let labels = labels_for(&source, "SELECT * FROM as");
        assert!(labels.iter().any(|label| label == "wms_asn"), "{labels:?}");

        // ...and a loose subsequence still finds it (w…s…n).
        let labels = labels_for(&source, "SELECT * FROM wsn");
        assert_eq!(
            labels.first().map(String::as_str),
            Some("wms_asn"),
            "{labels:?}"
        );

        // An identifier that shares nothing with the name is not offered.
        let labels = labels_for(&source, "SELECT * FROM zzz");
        assert!(!labels.iter().any(|label| label == "wms_asn"), "{labels:?}");
    }

    #[test]
    fn the_everyday_keyword_leads_the_table_names() {
        // Navicat's order: what the user is writing leads the objects.
        let source = catalog_source(&["sequence", "user_search"], &[]);
        let labels = labels_for(&source, "SELECT * FROM se");
        assert_eq!(
            labels.first().map(String::as_str),
            Some("SELECT"),
            "`se` must lead with SELECT, not the `sequence` table: {labels:?}"
        );
        let table = labels.iter().position(|label| label == "sequence");
        let keyword = labels.iter().position(|label| label == "SEED");
        assert!(
            matches!((table, keyword), (Some(table), Some(keyword)) if table < keyword),
            "the table names follow the everyday keywords: {labels:?}"
        );
    }

    #[test]
    fn a_field_of_the_statements_own_table_leads_the_keywords() {
        // `WHERE orde` on an `orders` table: the column is what the caret is asking for, so the
        // `ORDER` keyword must not take its place.
        let source = catalog_source(&["orders"], &[("orders", &["order_no", "status"])]);
        let labels = labels_for(&source, "SELECT * FROM orders WHERE orde");
        assert_eq!(
            labels.first().map(String::as_str),
            Some("order_no"),
            "{labels:?}"
        );
        let column = labels.iter().position(|label| label == "order_no");
        let keyword = labels.iter().position(|label| label == "ORDER");
        assert!(
            matches!((column, keyword), (Some(column), Some(keyword)) if column < keyword),
            "the statement's own fields lead the keywords: {labels:?}"
        );
    }

    #[test]
    fn a_keyword_outranks_a_function_family_sharing_its_prefix() {
        let source = empty_catalog_source();
        let sql = "SELECT * FROM fro";
        let context = crate::sql::completion_context(sql, sql.len(), DriverDialect::Mysql);
        let labels: Vec<String> = build_items(&source, &context)
            .into_iter()
            .map(|item| item.label)
            .collect();
        let from = labels.iter().position(|label| label == "FROM");
        let base64 = labels.iter().position(|label| label == "FROM_BASE64");
        assert!(
            matches!(from, Some(from) if matches!(base64, Some(base64) if from < base64)),
            "the FROM keyword must sort before FROM_BASE64: {labels:?}"
        );
    }

    #[test]
    fn a_common_keyword_leads_the_words_that_share_its_prefix() {
        let source = empty_catalog_source();
        // The curated list's order decides between two everyday keywords...
        let sql = "SELECT * se";
        let context = crate::sql::completion_context(sql, sql.len(), DriverDialect::Mysql);
        let labels: Vec<String> = build_items(&source, &context)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(
            labels.first().map(String::as_str),
            Some("SELECT"),
            "`se` must lead with SELECT, not SET: {labels:?}"
        );

        // ...and between an everyday keyword and the rest of the language.
        let sql = "SELECT * fr";
        let context = crate::sql::completion_context(sql, sql.len(), DriverDialect::Mysql);
        let labels: Vec<String> = build_items(&source, &context)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(
            labels.first().map(String::as_str),
            Some("FROM"),
            "`fr` must lead with FROM, not FRAME_ROW/FREE/FREEZE: {labels:?}"
        );
        // The rest of the family follows, closest first: the fewest extra characters to type.
        let position = |label: &str| {
            labels
                .iter()
                .position(|candidate| candidate == label)
                .unwrap_or_else(|| panic!("{label} is offered for `fr`: {labels:?}"))
        };
        assert!(
            position("FREE") < position("FREEZE") && position("FREEZE") < position("FRAME_ROW"),
            "the closer match leads within the group: {labels:?}"
        );
    }
}
