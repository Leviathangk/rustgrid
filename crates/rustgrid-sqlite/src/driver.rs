use std::time::Duration;

use async_trait::async_trait;
use rustgrid_core::{
    AdvancedFields, AutoIncrementRule, Connection, ConnectionConfig, ConnectionFieldLabel,
    ConnectionFormSpec, ConnectionHomePage, ConnectionPage, ConnectionStandardField,
    DatabaseEditorSpec, DesignerColumnOption, DesignerFieldColumn, DesignerIndexColumn,
    DesignerIndexKind, DesignerTab, Driver, DriverCapabilities, DriverDescriptor, DriverDialect,
    DriverIconStyle, DriverId, Error, Result, TableDesignerSpec,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::connection::SqliteConnection;

/// The SQLite driver. SQLite is a file-based engine: the database is a single file and there is no
/// server, user or password. The profile's `database` field holds the file path.
#[derive(Debug, Default)]
pub struct SqliteDriver;

impl SqliteDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Driver for SqliteDriver {
    fn id(&self) -> DriverId {
        DriverId::new("sqlite")
    }

    fn display_name(&self) -> String {
        "SQLite".to_string()
    }

    fn default_port(&self) -> u16 {
        0
    }

    fn is_file_based(&self) -> bool {
        true
    }

    fn supports_database_management(&self) -> bool {
        false
    }

    fn supports_users(&self) -> bool {
        false
    }

    fn supports_routines(&self) -> bool {
        false
    }

    fn database_editor(&self) -> DatabaseEditorSpec {
        DatabaseEditorSpec::default()
    }

    fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor {
            // SQLite keeps no table options and no table comment, and its designer DDL emits no
            // per-column collation, so those tabs and controls are left off rather than shown dead.
            table_designer: TableDesignerSpec {
                tabs: vec![
                    DesignerTab::Fields,
                    DesignerTab::Indexes,
                    DesignerTab::ForeignKeys,
                    DesignerTab::Sql,
                ],
                column_options: vec![
                    DesignerColumnOption::Default,
                    DesignerColumnOption::AutoIncrement,
                ],
                field_columns: vec![
                    DesignerFieldColumn::Length,
                    DesignerFieldColumn::NotNull,
                    DesignerFieldColumn::Generated,
                    DesignerFieldColumn::Key,
                ],
                option_fields: Vec::new(),
                default_suggestions: vec![
                    "CURRENT_TIMESTAMP",
                    "CURRENT_DATE",
                    "CURRENT_TIME",
                    "NULL",
                ],
                // `AUTOINCREMENT` only exists on an `INTEGER PRIMARY KEY`.
                auto_increment: AutoIncrementRule {
                    types: vec!["integer"],
                    ..AutoIncrementRule::default()
                },
                // SQLite indexes sort per column; there is no prefix length.
                index_columns: vec![DesignerIndexColumn::Sort],
                // `CREATE [UNIQUE] INDEX` is all SQLite has: no type of its own and no comment.
                index_kinds: vec![
                    DesignerIndexKind {
                        label: "NORMAL",
                        unique: false,
                        types: None,
                    },
                    DesignerIndexKind {
                        label: "UNIQUE",
                        unique: true,
                        types: None,
                    },
                ],
                index_comment: false,
            },
            id: DriverId::new("sqlite"),
            display_name: "SQLite".to_string(),
            default_port: 0,
            is_file_based: true,
            icon: "icons/sqlite.svg",
            // A slightly brighter blue than the logo's #003b57 so it stays legible on dark themes.
            icon_style: DriverIconStyle::Brand(0x0f80cc),
            // SQLite has no databases, server accounts, routines or schemas of its own.
            capabilities: DriverCapabilities::none(),
            database_editor: DatabaseEditorSpec::default(),
            // A file-based engine: the 常规 page edits one database file, and there is no TLS or
            // tunnel (a local file has no network endpoint), only 高级.
            connection_form: ConnectionFormSpec {
                home: ConnectionHomePage::File,
                tabs: vec![ConnectionPage::Advanced],
                // SQLite's "connect timeout" is really the lock / busy timeout: relabel it so it
                // does not read as a TCP handshake.
                labels: vec![ConnectionFieldLabel {
                    field: ConnectionStandardField::ConnectTimeout,
                    label_key: "form.advanced.busy_timeout",
                    hint_key: Some("form.advanced.busy_timeout_hint"),
                    visible_when: None,
                }],
                options: Vec::new(),
                // SQLite reads the connect timeout (as a busy timeout), the read-only flag and the
                // init SQL; it has no query timeout or keep-alive.
                advanced: AdvancedFields {
                    connect_timeout: true,
                    read_only: true,
                    init_sql: true,
                    ..AdvancedFields::NONE
                },
            },
            order: 40,
        }
    }

    fn dialect(&self) -> DriverDialect {
        DriverDialect::Sqlite
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        // The file path is carried in `database`; fall back to `host` so an older/hand-made profile
        // (or a copied MySQL one) still resolves.
        let path = config
            .database
            .clone()
            .filter(|path| !path.trim().is_empty())
            .or_else(|| (!config.host.trim().is_empty()).then(|| config.host.trim().to_string()))
            .ok_or_else(|| Error::Connection("a SQLite database file is required".to_string()))?;

        let mut options = SqliteConnectOptions::new()
            .filename(&path)
            // Only open an existing file. Creating a new database is a separate, explicit flow so a
            // typo cannot silently produce an empty file.
            .create_if_missing(false)
            .foreign_keys(true)
            .read_only(config.settings.read_only);
        if let Some(seconds) = config
            .settings
            .connect_timeout
            .filter(|seconds| *seconds > 0)
        {
            // SQLite has no connection timeout; the busy timeout is the closest equivalent (how
            // long it waits on a locked database before failing).
            options = options.busy_timeout(Duration::from_secs(seconds));
        }

        let init_sql = config.settings.init_sql.trim().to_string();
        let pool_options = SqlitePoolOptions::new().max_connections(5);
        let pool_options = if init_sql.is_empty() {
            pool_options
        } else {
            pool_options.after_connect(move |connection, _metadata| {
                let init_sql = init_sql.clone();
                Box::pin(async move {
                    sqlx::query(sqlx::AssertSqlSafe(init_sql))
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
        };

        let pool = pool_options
            .connect_with(options)
            .await
            .map_err(map_connect_error)?;

        Ok(Box::new(SqliteConnection::new(pool)))
    }
}

fn map_connect_error(error: sqlx::Error) -> Error {
    Error::Connection(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SQLite relabels the 高级 connect timeout as its lock / busy timeout and honors it (plus the
    /// read-only flag and the init SQL); it must not offer the fields it cannot apply.
    #[test]
    fn connection_form_relabels_the_busy_timeout_and_gates_the_advanced_fields() {
        let descriptor = SqliteDriver::new().descriptor();
        let form = &descriptor.connection_form;

        let label = form
            .labels
            .iter()
            .find(|label| label.field == ConnectionStandardField::ConnectTimeout)
            .expect("the connect timeout should be relabelled");
        assert_eq!(label.label_key, "form.advanced.busy_timeout");
        assert!(label.hint_key.is_some());

        assert!(form.advanced.connect_timeout);
        assert!(form.advanced.read_only);
        assert!(form.advanced.init_sql);
        assert!(!form.advanced.query_timeout);
        assert!(!form.advanced.keepalive);
    }
}
