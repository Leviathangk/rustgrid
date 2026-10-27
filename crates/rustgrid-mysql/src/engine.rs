//! The engine flavour behind the MySQL-protocol drivers.
//!
//! MySQL and MariaDB speak the same wire protocol and share almost all catalog and DDL syntax, so
//! one [`crate::MysqlConnection`] implementation serves both. This enum carries the handful of
//! genuine differences (driver id, session-variable names, ...).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    MySql,
    MariaDb,
    /// StarRocks speaks the MySQL wire protocol but rejects parts of MySQL's session/DDL syntax
    /// (its `SET sql_mode` login, bound LIMIT/OFFSET and prepared DML). It is a mode of the MySQL
    /// driver (chosen by the connection's compatibility option), not a separate profile driver.
    StarRocks,
}

impl Engine {
    /// The registry id of the driver that produces this engine.
    pub fn driver_id(self) -> &'static str {
        match self {
            Engine::MySql | Engine::StarRocks => "mysql",
            Engine::MariaDb => "mariadb",
        }
    }

    /// The `SET SESSION` statement that caps a single statement's execution time. MySQL expresses
    /// its limit in milliseconds (`max_execution_time`); MariaDB uses seconds
    /// (`max_statement_time`), and StarRocks uses seconds (`query_timeout`).
    pub fn statement_timeout_sql(self, seconds: u64) -> String {
        match self {
            Engine::MySql => format!(
                "SET SESSION max_execution_time = {}",
                seconds.saturating_mul(1000)
            ),
            Engine::MariaDb => format!("SET SESSION max_statement_time = {seconds}"),
            Engine::StarRocks => format!("SET SESSION query_timeout = {seconds}"),
        }
    }
}
