//! Engine-agnostic schema and row comparison for the 数据传输/同步 workbench.
//!
//! The workbench reads both sides through [`Connection`](crate::Connection) and hands the decoded
//! rows to [`diff_rows`]. The resulting [`RowChange`]s are applied through the target connection's
//! ordinary `insert_rows` / `update_rows` / `delete_rows` methods, so this module never needs to
//! know an engine's DML dialect. [`render_change_sql`] exists only for the SQL preview.

use crate::dialect::DriverDialect;
use crate::model::{CellValue, ColumnDef, ColumnInfo};

/// How one column differs between the source and target table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaChangeKind {
    /// The source has the column and the target does not.
    Added,
    /// The target has the column and the source does not.
    Removed,
    /// Both have the column, but its definition differs.
    Modified,
}

/// One structural difference between a source and a target table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaChange {
    pub kind: SchemaChangeKind,
    pub column: String,
    /// The source column's rendered type (empty for [`SchemaChangeKind::Removed`]).
    pub source_type: String,
    /// The target column's rendered type (empty for [`SchemaChangeKind::Added`]).
    pub target_type: String,
}

impl SchemaChange {
    /// A one-line description such as `email varchar(255) → varchar(120)`.
    pub fn summary(&self) -> String {
        match self.kind {
            SchemaChangeKind::Added => format!("+ {} {}", self.column, self.source_type),
            SchemaChangeKind::Removed => format!("- {} {}", self.column, self.target_type),
            SchemaChangeKind::Modified => format!(
                "~ {} {} → {}",
                self.column, self.target_type, self.source_type
            ),
        }
    }
}

/// The rendered type of a designer column (`varchar(255)`, `decimal(10,2) unsigned`, ...).
pub fn column_type_text(column: &ColumnDef) -> String {
    let mut text = column.data_type.trim().to_string();
    let length = column.length.trim();
    let decimals = column.decimals.trim();
    if !length.is_empty() && length != "0" {
        text.push('(');
        text.push_str(length);
        if !decimals.is_empty() && decimals != "0" {
            text.push(',');
            text.push_str(decimals);
        }
        text.push(')');
    }
    if column.unsigned {
        text.push_str(" unsigned");
    }
    text
}

/// Compare two table definitions, returning every added, removed or modified column.
///
/// Column names are matched case-insensitively (engines differ in how they fold unquoted
/// identifiers). Only definition fields that affect stored data are compared; a comment-only
/// change is not reported.
pub fn diff_columns(source: &[ColumnDef], target: &[ColumnDef]) -> Vec<SchemaChange> {
    let mut changes = Vec::new();
    for column in source {
        match target
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(&column.name))
        {
            None => changes.push(SchemaChange {
                kind: SchemaChangeKind::Added,
                column: column.name.clone(),
                source_type: column_type_text(column),
                target_type: String::new(),
            }),
            Some(existing) => {
                let source_type = column_type_text(column);
                let target_type = column_type_text(existing);
                let differs = source_type != target_type
                    || column.nullable != existing.nullable
                    || column.primary_key != existing.primary_key
                    || column.auto_increment != existing.auto_increment
                    || column.default != existing.default
                    || column.generated != existing.generated;
                if differs {
                    changes.push(SchemaChange {
                        kind: SchemaChangeKind::Modified,
                        column: column.name.clone(),
                        source_type,
                        target_type,
                    });
                }
            }
        }
    }
    for column in target {
        if !source
            .iter()
            .any(|candidate| candidate.name.eq_ignore_ascii_case(&column.name))
        {
            changes.push(SchemaChange {
                kind: SchemaChangeKind::Removed,
                column: column.name.clone(),
                source_type: String::new(),
                target_type: column_type_text(column),
            });
        }
    }
    changes
}

/// The kind of change a row needs on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowChangeKind {
    Insert,
    Update,
    Delete,
}

impl RowChangeKind {
    /// The i18n key of the row's action badge.
    pub fn label_key(self) -> &'static str {
        match self {
            RowChangeKind::Insert => "sync.action.insert",
            RowChangeKind::Update => "sync.action.update",
            RowChangeKind::Delete => "sync.action.delete",
        }
    }
}

/// One changed column of an updated row. `None` is SQL `NULL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    pub column: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// One row-level difference between the source and target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowChange {
    pub kind: RowChangeKind,
    /// The row's key columns, used by the `WHERE` clause of an update/delete.
    pub keys: Vec<(String, Option<String>)>,
    /// The values to write: every copied column for an insert, the changed columns for an update.
    pub values: Vec<(String, Option<String>)>,
    /// The changed columns with both values, for the inspector's diff display.
    pub changes: Vec<CellChange>,
}

/// The row-level comparison of one table.
#[derive(Debug, Clone, Default)]
pub struct DataDiff {
    /// The key columns the comparison used. Empty when the table has no primary key.
    pub key_columns: Vec<String>,
    pub rows: Vec<RowChange>,
    pub inserts: usize,
    pub updates: usize,
    pub deletes: usize,
    /// Whether one side hit the comparison row cap, so the diff is incomplete.
    pub truncated: bool,
}

impl DataDiff {
    /// The total number of changed rows.
    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Compare the rows of a source and target table by primary key.
///
/// `key_columns` are the source's primary-key column names; a table without one yields an empty
/// diff (the UI offers a full overwrite instead). A row present only in the source becomes an
/// insert, a row present on both sides with differing values becomes an update, and a row present
/// only in the target becomes a delete. `truncated` records that the caller hit its row cap, so the
/// UI can say the comparison is partial.
pub fn diff_rows(
    source_columns: &[ColumnInfo],
    source_rows: &[Vec<CellValue>],
    target_columns: &[ColumnInfo],
    target_rows: &[Vec<CellValue>],
    key_columns: &[String],
    truncated: bool,
) -> DataDiff {
    let mut diff = DataDiff {
        key_columns: key_columns.to_vec(),
        truncated,
        ..Default::default()
    };
    if key_columns.is_empty() {
        return diff;
    }
    let Some(source_indexes) = key_indexes(source_columns, key_columns) else {
        return diff;
    };
    // A table the target does not have (yet) has no columns to key on: every source row is an
    // insert. Once the target does have rows, a missing key column means the two sides are not
    // comparable, so report nothing rather than guessing.
    let target_indexes = match key_indexes(target_columns, key_columns) {
        Some(indexes) => Some(indexes),
        None if target_rows.is_empty() => None,
        None => return diff,
    };

    let mut target_by_key = std::collections::HashMap::with_capacity(target_rows.len());
    if let Some(target_indexes) = target_indexes.as_ref() {
        for (index, row) in target_rows.iter().enumerate() {
            if let Some(key) = key_of(row, target_indexes) {
                target_by_key.insert(key, index);
            }
        }
    }

    let mut seen = std::collections::HashSet::with_capacity(source_rows.len());
    for source_row in source_rows.iter() {
        let Some(key) = key_of(source_row, &source_indexes) else {
            continue;
        };
        seen.insert(key.clone());
        match target_by_key.get(&key) {
            None => {
                diff.inserts += 1;
                diff.rows.push(RowChange {
                    kind: RowChangeKind::Insert,
                    keys: row_keys(source_columns, source_row, key_columns),
                    values: mapped_values(source_columns, target_columns, source_row),
                    changes: Vec::new(),
                });
            }
            Some(target_index) => {
                let Some(target_row) = target_rows.get(*target_index) else {
                    continue;
                };
                let mut changes = Vec::new();
                let mut values = Vec::new();
                for (source_position, column) in source_columns.iter().enumerate() {
                    let Some(target_position) = find_column(target_columns, &column.name) else {
                        continue;
                    };
                    let before = display_value(target_row.get(target_position));
                    let after = display_value(source_row.get(source_position));
                    if before == after {
                        continue;
                    }
                    values.push((
                        column.name.clone(),
                        source_row
                            .get(source_position)
                            .and_then(|value| value.as_edit_value()),
                    ));
                    changes.push(CellChange {
                        column: column.name.clone(),
                        before,
                        after,
                    });
                }
                if !changes.is_empty() {
                    diff.updates += 1;
                    diff.rows.push(RowChange {
                        kind: RowChangeKind::Update,
                        keys: row_keys(source_columns, source_row, key_columns),
                        values,
                        changes,
                    });
                }
            }
        }
    }

    for target_row in target_rows {
        let Some(target_indexes) = target_indexes.as_ref() else {
            break;
        };
        let Some(key) = key_of(target_row, target_indexes) else {
            continue;
        };
        if seen.contains(&key) {
            continue;
        }
        diff.deletes += 1;
        diff.rows.push(RowChange {
            kind: RowChangeKind::Delete,
            keys: row_keys(target_columns, target_row, key_columns),
            values: Vec::new(),
            changes: Vec::new(),
        });
    }
    diff
}

/// Render one row change as a single SQL statement, for the inspector's preview.
///
/// The values are the edit strings the connection methods would bind, so the preview shows the
/// same data the run writes. `None` renders as `NULL`; string values are quoted per `dialect`.
pub fn render_change_sql(table: &str, change: &RowChange, dialect: DriverDialect) -> String {
    let target = dialect.quote_identifier(table);
    match change.kind {
        RowChangeKind::Insert => {
            let columns = change
                .values
                .iter()
                .map(|(column, _)| dialect.quote_identifier(column))
                .collect::<Vec<_>>()
                .join(", ");
            let values = change
                .values
                .iter()
                .map(|(_, value)| render_literal(value, dialect))
                .collect::<Vec<_>>()
                .join(", ");
            format!("INSERT INTO {target} ({columns}) VALUES ({values});")
        }
        RowChangeKind::Update => {
            let assignments = change
                .values
                .iter()
                .map(|(column, value)| {
                    format!(
                        "{} = {}",
                        dialect.quote_identifier(column),
                        render_literal(value, dialect)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "UPDATE {target} SET {assignments} WHERE {};",
                render_keys(&change.keys, dialect)
            )
        }
        RowChangeKind::Delete => format!(
            "DELETE FROM {target} WHERE {};",
            render_keys(&change.keys, dialect)
        ),
    }
}

/// Render a bind value as a SQL literal for the preview.
pub fn render_literal(value: &Option<String>, dialect: DriverDialect) -> String {
    let Some(text) = value else {
        return "NULL".to_string();
    };
    match dialect {
        DriverDialect::Mysql => quote_string(text, true),
        DriverDialect::Sqlite => quote_string(text, false),
        DriverDialect::SqlServer => format!("N{}", quote_string(text, false)),
        DriverDialect::Postgres | DriverDialect::Oracle | DriverDialect::Generic => {
            quote_string(text, false)
        }
    }
}

fn quote_string(value: &str, escape_backslash: bool) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for character in value.chars() {
        match character {
            '\'' => out.push_str("''"),
            '\\' if escape_backslash => out.push_str("\\\\"),
            '\0' if escape_backslash => out.push_str("\\0"),
            '\n' if escape_backslash => out.push_str("\\n"),
            '\r' if escape_backslash => out.push_str("\\r"),
            '\t' if escape_backslash => out.push_str("\\t"),
            _ => out.push(character),
        }
    }
    out.push('\'');
    out
}

fn render_keys(keys: &[(String, Option<String>)], dialect: DriverDialect) -> String {
    if keys.is_empty() {
        return "1 = 0".to_string();
    }
    keys.iter()
        .map(|(column, value)| match value {
            None => format!("{} IS NULL", dialect.quote_identifier(column)),
            Some(_) => format!(
                "{} = {}",
                dialect.quote_identifier(column),
                render_literal(value, dialect)
            ),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn find_column(columns: &[ColumnInfo], name: &str) -> Option<usize> {
    columns
        .iter()
        .position(|column| column.name == name)
        .or_else(|| {
            columns
                .iter()
                .position(|column| column.name.eq_ignore_ascii_case(name))
        })
}

fn key_indexes(columns: &[ColumnInfo], keys: &[String]) -> Option<Vec<usize>> {
    keys.iter().map(|key| find_column(columns, key)).collect()
}

/// A comparable, collision-resistant representation of one row's key. `NULL` and an empty string
/// are different keys.
fn key_of(row: &[CellValue], indexes: &[usize]) -> Option<String> {
    let mut key = String::new();
    for index in indexes {
        let value = row.get(*index)?;
        key.push('\u{1f}');
        match value.as_edit_value() {
            Some(text) => {
                key.push('v');
                key.push_str(&text);
            }
            None => key.push('n'),
        }
    }
    Some(key)
}

fn row_keys(
    columns: &[ColumnInfo],
    row: &[CellValue],
    keys: &[String],
) -> Vec<(String, Option<String>)> {
    keys.iter()
        .filter_map(|name| {
            let index = find_column(columns, name)?;
            let value = row.get(index)?.as_edit_value();
            Some((name.clone(), value))
        })
        .collect()
}

fn mapped_values(
    source_columns: &[ColumnInfo],
    target_columns: &[ColumnInfo],
    row: &[CellValue],
) -> Vec<(String, Option<String>)> {
    source_columns
        .iter()
        .enumerate()
        .filter(|(_, column)| find_column(target_columns, &column.name).is_some())
        .filter_map(|(index, column)| {
            let value = row.get(index)?.as_edit_value();
            Some((column.name.clone(), value))
        })
        .collect()
}

fn display_value(value: Option<&CellValue>) -> Option<String> {
    match value {
        None | Some(CellValue::Null) => None,
        Some(value) => Some(value.as_display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str, primary_key: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: !primary_key,
            primary_key,
            comment: String::new(),
        }
    }

    fn columns() -> Vec<ColumnInfo> {
        vec![
            column("id", "bigint", true),
            column("email", "varchar(255)", false),
            column("role", "varchar(32)", false),
        ]
    }

    fn row(id: i64, email: &str, role: &str) -> Vec<CellValue> {
        vec![
            CellValue::Int(id),
            CellValue::Text(email.to_string()),
            CellValue::Text(role.to_string()),
        ]
    }

    #[test]
    fn row_diff_finds_inserts_updates_and_deletes() {
        let source = vec![
            row(1, "a@example.com", "admin"),
            row(2, "b@example.com", "member"),
        ];
        let target = vec![
            row(1, "a@example.com", "member"),
            row(3, "gone@example.com", "member"),
        ];
        let diff = diff_rows(
            &columns(),
            &source,
            &columns(),
            &target,
            &["id".to_string()],
            false,
        );
        assert_eq!(diff.inserts, 1);
        assert_eq!(diff.updates, 1);
        assert_eq!(diff.deletes, 1);
        assert_eq!(diff.total(), 3);
        let update = diff
            .rows
            .iter()
            .find(|change| change.kind == RowChangeKind::Update)
            .expect("update");
        assert_eq!(update.keys, vec![("id".to_string(), Some("1".to_string()))]);
        assert_eq!(
            update.values,
            vec![("role".to_string(), Some("admin".to_string()))]
        );
        assert_eq!(update.changes[0].before.as_deref(), Some("member"));
        assert_eq!(update.changes[0].after.as_deref(), Some("admin"));
    }

    #[test]
    fn a_missing_target_table_makes_every_row_an_insert() {
        let source = vec![row(1, "a@example.com", "admin")];
        let diff = diff_rows(&columns(), &source, &[], &[], &["id".to_string()], false);
        assert_eq!(diff.inserts, 1);
        assert_eq!(diff.updates, 0);
        assert_eq!(diff.deletes, 0);
    }

    #[test]
    fn row_diff_requires_a_key() {
        let diff = diff_rows(
            &columns(),
            &[row(1, "a", "b")],
            &columns(),
            &[row(2, "c", "d")],
            &[],
            false,
        );
        assert!(diff.is_empty());
        assert!(diff.key_columns.is_empty());
    }

    #[test]
    fn row_diff_treats_null_and_empty_string_as_distinct() {
        let columns = vec![column("id", "bigint", true), column("note", "text", false)];
        let source = vec![vec![CellValue::Int(1), CellValue::Null]];
        let target = vec![vec![CellValue::Int(1), CellValue::Text(String::new())]];
        let diff = diff_rows(
            &columns,
            &source,
            &columns,
            &target,
            &["id".to_string()],
            false,
        );
        assert_eq!(diff.updates, 1);
        assert_eq!(diff.rows[0].values[0].1, None);
    }

    #[test]
    fn schema_diff_reports_added_removed_and_modified_columns() {
        let source = vec![
            ColumnDef {
                name: "id".to_string(),
                data_type: "bigint".to_string(),
                primary_key: true,
                ..Default::default()
            },
            ColumnDef {
                name: "email".to_string(),
                data_type: "varchar".to_string(),
                length: "255".to_string(),
                ..Default::default()
            },
            ColumnDef {
                name: "mfa".to_string(),
                data_type: "varchar".to_string(),
                length: "64".to_string(),
                ..Default::default()
            },
        ];
        let target = vec![
            ColumnDef {
                name: "id".to_string(),
                data_type: "bigint".to_string(),
                primary_key: true,
                ..Default::default()
            },
            ColumnDef {
                name: "email".to_string(),
                data_type: "varchar".to_string(),
                length: "120".to_string(),
                ..Default::default()
            },
            ColumnDef {
                name: "legacy".to_string(),
                data_type: "int".to_string(),
                ..Default::default()
            },
        ];
        let changes = diff_columns(&source, &target);
        assert!(
            changes
                .iter()
                .any(|change| { change.kind == SchemaChangeKind::Added && change.column == "mfa" })
        );
        assert!(changes.iter().any(|change| {
            change.kind == SchemaChangeKind::Removed && change.column == "legacy"
        }));
        assert!(changes.iter().any(|change| {
            change.kind == SchemaChangeKind::Modified && change.column == "email"
        }));
    }

    #[test]
    fn sql_preview_quotes_identifiers_and_literals() {
        let change = RowChange {
            kind: RowChangeKind::Update,
            keys: vec![("id".to_string(), Some("7".to_string()))],
            values: vec![("name".to_string(), Some("O'Brien".to_string()))],
            changes: Vec::new(),
        };
        let sql = render_change_sql("users", &change, DriverDialect::Mysql);
        assert_eq!(
            sql,
            "UPDATE `users` SET `name` = 'O''Brien' WHERE `id` = '7';"
        );
        let delete = RowChange {
            kind: RowChangeKind::Delete,
            keys: vec![("id".to_string(), None)],
            values: Vec::new(),
            changes: Vec::new(),
        };
        assert_eq!(
            render_change_sql("users", &delete, DriverDialect::SqlServer),
            "DELETE FROM [users] WHERE [id] IS NULL;"
        );
    }
}
