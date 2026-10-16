use async_trait::async_trait;
use rustgrid_core::{
    AdvancedFields, Connection, ConnectionConfig, ConnectionFormSpec, ConnectionHomePage,
    DatabaseEditorSpec, Driver, DriverCapabilities, DriverDescriptor, DriverDialect,
    DriverIconStyle, DriverId, Error, Result, TableDesignerSpec,
};

use crate::api;
use crate::connection::OdbcConnection;

/// The generic ODBC driver. It connects through a user-installed ODBC driver (or a raw connection
/// string) and speaks the engine's SQL, so it is the fallback for engines RustGrid has no native
/// Rust driver for.
#[derive(Debug, Default)]
pub struct OdbcDriver;

impl OdbcDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Driver for OdbcDriver {
    fn id(&self) -> DriverId {
        DriverId::new("odbc")
    }

    fn display_name(&self) -> String {
        "ODBC".to_string()
    }

    fn default_port(&self) -> u16 {
        0
    }

    fn is_file_based(&self) -> bool {
        false
    }

    // The capabilities below are engine-dependent for ODBC; the conservative defaults keep the
    // engine-specific tabs (Users/Routines/schema management) hidden.
    fn supports_database_management(&self) -> bool {
        false
    }

    fn supports_users(&self) -> bool {
        false
    }

    fn supports_routines(&self) -> bool {
        false
    }

    fn supports_schemas(&self) -> bool {
        false
    }

    fn database_editor(&self) -> DatabaseEditorSpec {
        DatabaseEditorSpec::default()
    }

    fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor {
            // The generic ODBC driver does not implement the table designer at all, so it keeps the
            // default shape; the panel is never opened for it.
            table_designer: TableDesignerSpec::default(),
            id: DriverId::new("odbc"),
            display_name: "ODBC".to_string(),
            default_port: 0,
            is_file_based: false,
            icon: "icons/connection.svg",
            icon_style: DriverIconStyle::Plain,
            capabilities: DriverCapabilities::none(),
            database_editor: DatabaseEditorSpec::default(),
            connection_form: ConnectionFormSpec {
                // The generic ODBC driver is the one engine whose form IS the plain form: the
                // ODBC fields on the 常规 page, nothing engine-specific beyond that. It reads no
                // TLS, tunnel or advanced settings (they all live inside its connection string),
                // so 常规 is the only page.
                home: ConnectionHomePage::Odbc,
                tabs: Vec::new(),
                advanced: AdvancedFields::NONE,
                ..Default::default()
            },
            order: 80,
        }
    }

    fn dialect(&self) -> DriverDialect {
        DriverDialect::Generic
    }

    fn connection_drivers(&self) -> Vec<String> {
        api::api()
            .ok()
            .and_then(|api| api.driver_descriptions().ok())
            .unwrap_or_default()
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let connection_string = connection_string(config)?;
        let api = api::api().map_err(Error::Connection)?;
        let handle = api
            .connect(&connection_string)
            .map_err(classify_connect_error)?;
        Ok(Box::new(OdbcConnection::new(handle, config)))
    }
}

/// Build the ODBC connection string from a profile. A full connection string wins; otherwise a DSN
/// plus credentials, or a driver name plus the profile's server/credentials.
fn connection_string(config: &ConnectionConfig) -> Result<String> {
    if let Some(value) = config
        .options
        .get("odbc.connection_string")
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(value.clone());
    }

    let password = escape_attribute_value(config.password.as_deref().unwrap_or(""));

    if let Some(dsn) = config
        .options
        .get("odbc.dsn")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return Ok(format!(
            "DSN={dsn};UID={};PWD={password};",
            config.username.trim()
        ));
    }

    let driver = config
        .options
        .get("odbc.driver")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let Some(driver) = driver else {
        return Err(Error::Connection(
            "an ODBC driver name or connection string is required".to_string(),
        ));
    };

    // The engine hint the user set is free text ("sqlserver"), so every match below is loose.
    let engine = config
        .options
        .get("odbc.engine")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();

    let mut parts = vec![format!("Driver={{{driver}}}")];
    let host = config.host.trim();
    if !host.is_empty() {
        // Where the port goes depends on the driver. SQL Server's ODBC drivers want it inside the
        // server attribute (`Server=host,port`); for every other driver a comma there means a
        // *list* — MySQL Connector/ODBC ≥ 8.0.28 reads `Server=host,port` as two hosts and refuses
        // it with "Missing option MULTI_HOST=1", and psqlODBC reads the number as a second host
        // name — so those get the port as its own attribute (`Server=host;Port=port`).
        let port_in_server = config.port > 0 && server_takes_port(driver, &engine);
        let server = if port_in_server {
            format!("{host},{}", config.port)
        } else {
            host.to_string()
        };
        parts.push(format!("Server={server}"));
        if config.port > 0 && !port_in_server {
            parts.push(format!("Port={}", config.port));
        }
        // A hand-typed multi-host list is what MySQL's driver needs to be told about explicitly.
        if !port_in_server && host.contains(',') && is_mysql(driver, &engine) {
            parts.push("MULTI_HOST=1".to_string());
        }
    }
    if !config.username.trim().is_empty() {
        parts.push(format!("UID={}", config.username.trim()));
    }
    parts.push(format!("PWD={password}"));
    if let Some(database) = config
        .database
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("Database={}", database.trim()));
    }
    Ok(parts.join(";") + ";")
}

/// Escape an ODBC connection-string attribute value (a `}` inside a `{...}` value is doubled).
fn escape_attribute_value(value: &str) -> String {
    value.replace('}', "}}")
}

/// Whether the driver reads the port from the server attribute (`Server=host,port`), which is the
/// SQL Server ODBC drivers' convention. Every other driver takes the port as its own attribute.
fn server_takes_port(driver: &str, engine: &str) -> bool {
    let driver = driver.to_ascii_lowercase();
    [engine, driver.as_str()].iter().any(|name| {
        name.contains("sqlserver") || name.contains("sql server") || name.contains("mssql")
    })
}

/// Whether the driver is a MySQL/MariaDB one, whose `Server` may carry a comma-separated host list
/// only once `MULTI_HOST=1` is set (Connector/ODBC ≥ 8.0.28 rejects it otherwise).
fn is_mysql(driver: &str, engine: &str) -> bool {
    let driver = driver.to_ascii_lowercase();
    [engine, driver.as_str()]
        .iter()
        .any(|name| name.contains("mysql") || name.contains("mariadb"))
}

/// Turn a driver-manager error message into an authentication error when it looks like a login
/// failure, so the app offers the password prompt instead of a generic connection error.
fn classify_connect_error(message: String) -> Error {
    let lower = message.to_lowercase();
    if message.contains("28000")
        || lower.contains("login failed")
        || lower.contains("authentication")
        || lower.contains("access denied")
    {
        Error::Authentication(message)
    } else {
        Error::Connection(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use rustgrid_core::ConnectionOptions;

    /// A profile that fills the form's fields (no DSN, no connection string) for `driver`.
    fn config(driver: &str, host: &str, port: u16, engine: &str) -> ConnectionConfig {
        let mut options = BTreeMap::from([("odbc.driver".to_string(), driver.to_string())]);
        if !engine.is_empty() {
            options.insert("odbc.engine".to_string(), engine.to_string());
        }
        ConnectionConfig {
            driver: DriverId::new("odbc"),
            host: host.to_string(),
            port,
            username: "root".to_string(),
            password: Some("secret".to_string()),
            database: Some("app".to_string()),
            options,
            settings: ConnectionOptions::default(),
        }
    }

    #[test]
    fn mysql_takes_the_port_as_its_own_attribute() {
        // MySQL Connector/ODBC reads `Server=host,port` as a two-host failover list and rejects it
        // with "Missing option MULTI_HOST=1", so the port must be its own attribute.
        let string = connection_string(&config(
            "MySQL ODBC 8.0 Unicode Driver",
            "127.0.0.1",
            3306,
            "",
        ))
        .expect("build the connection string");
        assert_eq!(
            string,
            "Driver={MySQL ODBC 8.0 Unicode Driver};Server=127.0.0.1;Port=3306;UID=root;PWD=secret;Database=app;"
        );
    }

    #[test]
    fn the_engine_hint_is_enough_to_recognize_mysql() {
        // A renamed driver must still be recognized through 底层数据库类型.
        let string = connection_string(&config("Some Renamed Driver", "db.local", 3307, "MariaDB"))
            .expect("build the connection string");
        assert!(string.contains("Server=db.local;Port=3307;"), "{string}");
    }

    #[test]
    fn sqlserver_keeps_the_port_inside_the_server_attribute() {
        let string = connection_string(&config(
            "ODBC Driver 17 for SQL Server",
            "10.0.0.5",
            1433,
            "",
        ))
        .expect("build the connection string");
        assert!(
            string.contains("Server=10.0.0.5,1433;"),
            "SQL Server wants the port inside Server: {string}"
        );
        assert!(!string.contains("Port="), "{string}");
    }

    #[test]
    fn a_portless_profile_has_no_port_attribute() {
        let string = connection_string(&config("PostgreSQL Unicode", "db.local", 0, ""))
            .expect("build the connection string");
        assert!(string.contains("Server=db.local;"), "{string}");
        assert!(!string.contains("Port="), "{string}");
    }

    #[test]
    fn a_typed_multi_host_list_asks_mysql_for_multi_host() {
        let string = connection_string(&config(
            "MySQL ODBC 8.0 Unicode Driver",
            "db1.local,db2.local",
            3306,
            "",
        ))
        .expect("build the connection string");
        assert!(
            string.contains("Server=db1.local,db2.local;Port=3306;MULTI_HOST=1;"),
            "{string}"
        );
    }

    #[test]
    fn a_full_connection_string_wins_over_the_fields() {
        let mut profile = config("MySQL ODBC 8.0 Unicode Driver", "127.0.0.1", 3306, "");
        profile.options.insert(
            "odbc.connection_string".to_string(),
            "Driver={MySQL ODBC 8.0 ANSI Driver};Server=other;".to_string(),
        );
        let string = connection_string(&profile).expect("build the connection string");
        assert_eq!(string, "Driver={MySQL ODBC 8.0 ANSI Driver};Server=other;");
    }

    #[test]
    fn a_dsn_carries_the_credentials_alone() {
        let mut profile = config("MySQL ODBC 8.0 Unicode Driver", "127.0.0.1", 3306, "");
        profile
            .options
            .insert("odbc.dsn".to_string(), "erp".to_string());
        let string = connection_string(&profile).expect("build the connection string");
        assert_eq!(string, "DSN=erp;UID=root;PWD=secret;");
    }

    #[test]
    fn a_missing_driver_name_is_an_error() {
        let mut profile = config("MySQL ODBC 8.0 Unicode Driver", "127.0.0.1", 3306, "");
        profile.options.remove("odbc.driver");
        assert!(connection_string(&profile).is_err());
    }
}
