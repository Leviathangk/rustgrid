//! Regression: a table page must be ordered by the column's native type, not by the `::text`
//! projection alias. Ignored by default; enable with `RUSTGRID_PG_PASSWORD` (and optionally
//! `RUSTGRID_PG_HOST` / `PORT` / `USER` / `DATABASE`):
//!
//! ```text
//! RUSTGRID_PG_PASSWORD=... cargo test -p rustgrid-postgresql --test live_sort_order -- --ignored
//! ```

use rustgrid_core::{ConnectionConfig, Driver, DriverId, PageRequest, SortColumn, TablePage};
use rustgrid_postgresql::PostgresDriver;

fn config() -> ConnectionConfig {
    ConnectionConfig {
        driver: DriverId::new("postgresql"),
        host: std::env::var("RUSTGRID_PG_HOST").unwrap_or_else(|_| "localhost".to_string()),
        port: std::env::var("RUSTGRID_PG_PORT")
            .unwrap_or_else(|_| "5432".to_string())
            .parse()
            .expect("RUSTGRID_PG_PORT must be a number"),
        username: std::env::var("RUSTGRID_PG_USER").unwrap_or_else(|_| "postgres".to_string()),
        password: Some(std::env::var("RUSTGRID_PG_PASSWORD").unwrap_or_default()),
        database: Some(
            std::env::var("RUSTGRID_PG_DATABASE").unwrap_or_else(|_| "postgres".to_string()),
        ),
        options: Default::default(),
        settings: Default::default(),
    }
}

fn order(column: &str, descending: bool) -> PageRequest {
    PageRequest::new(0, 100).with_order_by(vec![SortColumn {
        column: column.to_string(),
        descending,
    }])
}

fn ids(page: &TablePage) -> Vec<String> {
    let index = page
        .columns
        .iter()
        .position(|column| column.name == "id")
        .expect("id column");
    page.rows
        .iter()
        .map(|row| row[index].as_display())
        .collect()
}

#[tokio::test]
#[ignore = "requires a live PostgreSQL (RUSTGRID_PG_*)"]
async fn table_pages_sort_by_native_types() {
    let config = config();
    let database = config
        .database
        .clone()
        .unwrap_or_else(|| "postgres".to_string());
    let connection = PostgresDriver::new()
        .connect(&config)
        .await
        .expect("connect");

    let table = "public.rustgrid_sort_order";
    for sql in [
        format!("DROP TABLE IF EXISTS {table}"),
        format!("CREATE TABLE {table} (id bigint PRIMARY KEY, amount numeric(12,2), name text)"),
        format!(
            "INSERT INTO {table} VALUES \
             (1, 9.5, 'a'), (2, 10.25, 'b'), (10, 100.00, 'c'), (20, 2.75, 'd')"
        ),
    ] {
        connection
            .execute_query(Some(&database), &sql)
            .await
            .expect("setup");
    }

    // Numeric id: text ordering yields 1, 10, 2, 20.
    let ascending = connection
        .fetch_page(&database, table, order("id", false))
        .await
        .expect("sort by id asc");
    assert_eq!(ids(&ascending), ["1", "2", "10", "20"]);

    let descending = connection
        .fetch_page(&database, table, order("id", true))
        .await
        .expect("sort by id desc");
    assert_eq!(ids(&descending), ["20", "10", "2", "1"]);

    // Decimal values: text ordering yields 10.25, 100.00, 2.75, 9.50.
    let by_amount = connection
        .fetch_page(&database, table, order("amount", false))
        .await
        .expect("sort by amount asc");
    assert_eq!(ids(&by_amount), ["20", "1", "2", "10"]);

    // No explicit sort: the page falls back to the primary key, which must also be native.
    let default_sort = connection
        .fetch_page(&database, table, PageRequest::new(0, 100))
        .await
        .expect("default sort by primary key");
    assert_eq!(ids(&default_sort), ["1", "2", "10", "20"]);

    let _ = connection
        .execute_query(Some(&database), &format!("DROP TABLE IF EXISTS {table}"))
        .await;
}
