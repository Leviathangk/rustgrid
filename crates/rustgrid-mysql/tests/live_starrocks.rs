use std::collections::BTreeMap;

use rustgrid_core::{
    CellValue, ConnectionConfig, Driver, DriverId, PageRequest, RowInsert, RowUpdate, TlsMode,
};
use rustgrid_mysql::MysqlDriver;

fn config() -> ConnectionConfig {
    let host = std::env::var("RUSTGRID_STARROCKS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("RUSTGRID_STARROCKS_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(9030);
    let username = std::env::var("RUSTGRID_STARROCKS_USER").unwrap_or_else(|_| "root".to_string());
    let password = std::env::var("RUSTGRID_STARROCKS_PASSWORD").ok();
    let mut options = BTreeMap::new();
    options.insert("mysql.compatibility".to_string(), "starrocks".to_string());
    let mut config = ConnectionConfig {
        driver: DriverId::new("mysql"),
        host,
        port,
        username,
        password,
        database: None,
        options,
        settings: Default::default(),
    };
    config.settings.tls.mode = TlsMode::Disabled;
    config.settings.query_timeout = Some(30);
    config
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live StarRocks server (set RUSTGRID_STARROCKS_PASSWORD if needed)"]
async fn live_starrocks_catalog_page_and_edit() {
    let driver = MysqlDriver::new();
    let connection = driver
        .connect(&config())
        .await
        .expect("connect to the live StarRocks server");

    let databases = connection.list_databases().await.expect("list databases");
    assert!(
        databases
            .iter()
            .any(|database| database.name == "information_schema"),
        "expected information_schema to be listed"
    );

    let database = "rustgrid_starrocks_live";
    connection
        .execute_query(None, &format!("CREATE DATABASE IF NOT EXISTS `{database}`"))
        .await
        .expect("create the scratch database");
    let table = "users";
    connection
        .execute_query(Some(database), &format!("DROP TABLE IF EXISTS `{table}`"))
        .await
        .expect("drop the scratch table");
    connection
        .execute_query(
            Some(database),
            &format!(
                "CREATE TABLE `{table}` (\
                 `id` BIGINT NOT NULL, \
                 `name` VARCHAR(64) NULL, \
                 `score` INT NULL) \
                 PRIMARY KEY (`id`) DISTRIBUTED BY HASH (`id`) BUCKETS 1 \
                 PROPERTIES ('replication_num' = '1')"
            ),
        )
        .await
        .expect("create the scratch table");

    connection
        .insert_rows(
            database,
            table,
            &[RowInsert {
                values: vec![
                    ("id".to_string(), Some("1".to_string())),
                    ("name".to_string(), Some("alice".to_string())),
                    ("score".to_string(), Some("10".to_string())),
                ],
            }],
        )
        .await
        .expect("insert through the text protocol");

    let page = connection
        .fetch_page(database, table, PageRequest::new(0, 10))
        .await
        .expect("fetch the first page");
    assert_eq!(page.columns.len(), 3);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.total_rows, Some(1));
    assert!(matches!(&page.rows[0][1], CellValue::Text(name) if name == "alice"));

    connection
        .update_rows(
            database,
            table,
            &[RowUpdate {
                set: vec![("name".to_string(), Some("bob".to_string()))],
                keys: vec![("id".to_string(), Some("1".to_string()))],
            }],
        )
        .await
        .expect("update through the text protocol");
    let page = connection
        .fetch_page(database, table, PageRequest::new(0, 10))
        .await
        .expect("fetch the updated page");
    assert!(matches!(&page.rows[0][1], CellValue::Text(name) if name == "bob"));

    connection
        .delete_rows(
            database,
            table,
            &[vec![("id".to_string(), Some("1".to_string()))]],
        )
        .await
        .expect("delete through the text protocol");
    let page = connection
        .fetch_page(database, table, PageRequest::new(0, 10))
        .await
        .expect("fetch the empty page");
    assert_eq!(page.rows.len(), 0);
    assert_eq!(page.total_rows, Some(0));

    connection.close().await.expect("close");
}
