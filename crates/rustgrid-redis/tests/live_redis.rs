//! Live Redis integration test. Ignored by default because it needs a real server.
//!
//! Enable it by setting `RUSTGRID_REDIS_PASSWORD` (and optionally
//! `RUSTGRID_REDIS_HOST` / `PORT` / `DATABASE` / `USER`) and running:
//!
//! ```text
//! cargo test -p rustgrid-redis -- --ignored
//! ```
//!
//! A throwaway server:
//!
//! ```text
//! docker run -d --name rg-redis -p 6379:6379 redis:7
//! ```
//!
//! It writes keys under a unique `rustgrid:test:<pid>` prefix and removes them at the end, so it is
//! safe against any database it is pointed at, but still: do not run it against production.

use rustgrid_core::{
    ConnectionConfig, Driver, DriverId, ObjectCreate, PageRequest, RowInsert, RowUpdate, Watch,
    WatchSpec,
};
use rustgrid_redis::{OPT_KEY_LIMIT, OPT_MODE, RedisDriver};

fn env(name: &str, fallback: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| fallback.to_string())
}

fn config() -> ConnectionConfig {
    let mut options = std::collections::BTreeMap::new();
    options.insert(OPT_MODE.to_string(), "standalone".to_string());
    options.insert(OPT_KEY_LIMIT.to_string(), "500".to_string());
    ConnectionConfig {
        driver: DriverId::new("redis"),
        host: env("RUSTGRID_REDIS_HOST", "localhost"),
        port: env("RUSTGRID_REDIS_PORT", "6379")
            .parse()
            .expect("RUSTGRID_REDIS_PORT must be a number"),
        username: env("RUSTGRID_REDIS_USER", ""),
        password: {
            let password = env("RUSTGRID_REDIS_PASSWORD", "");
            (!password.is_empty()).then_some(password)
        },
        database: Some(env("RUSTGRID_REDIS_DATABASE", "0")),
        options,
        settings: Default::default(),
    }
}

#[tokio::test]
#[ignore = "requires a live Redis (RUSTGRID_REDIS_*)"]
async fn round_trips_a_live_redis() {
    let config = config();
    let database = config.database.clone().unwrap_or_else(|| "0".to_string());
    let database = format!("db{database}");
    let connection = RedisDriver::new().connect(&config).await.expect("connect");

    let version = connection.server_version().await.unwrap();
    assert!(version.contains('.'), "unexpected version {version}");

    let databases = connection.list_databases().await.unwrap();
    assert!(
        databases.iter().any(|entry| entry.name == database),
        "database {database} should be listed"
    );

    let prefix = format!("rustgrid:test:{}", std::process::id());
    let hash = format!("{prefix}:hash");
    let list = format!("{prefix}:list");

    // Commands through the query editor, including multi-statement scripts.
    let results = connection
        .execute_query_many(
            Some(&database),
            &format!("HSET {hash} name rustgrid\nHSET {hash} version 1\nHGETALL {hash}"),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[2].rows.len(), 2);

    // The key list and its object status.
    let tables = connection.list_tables(&database).await.unwrap();
    assert!(tables.iter().any(|table| table.name == hash));
    let statuses = connection.table_statuses(&database).await.unwrap();
    let status = statuses
        .iter()
        .find(|(name, _)| name == &hash)
        .map(|(_, status)| status)
        .expect("status for the hash");
    assert_eq!(status.engine.as_deref(), Some("hash"));
    assert_eq!(status.rows, Some(2));

    // A hash key pages as field/value rows.
    let columns = connection.columns(&database, &hash).await.unwrap();
    assert_eq!(columns[0].name, "field");
    assert!(columns[0].primary_key);
    let page = connection
        .fetch_page(&database, &hash, PageRequest::new(1, 10))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.total_rows, Some(2));

    // Edit one field in place, then add a row.
    connection
        .update_rows(
            &database,
            &hash,
            &[RowUpdate {
                set: vec![("value".to_string(), Some("2".to_string()))],
                keys: vec![("field".to_string(), Some("version".to_string()))],
            }],
        )
        .await
        .unwrap();
    connection
        .insert_rows(
            &database,
            &hash,
            &[RowInsert {
                values: vec![
                    ("field".to_string(), Some("new".to_string())),
                    ("value".to_string(), Some("yes".to_string())),
                ],
            }],
        )
        .await
        .unwrap();
    let page = connection
        .fetch_page(&database, &hash, PageRequest::new(1, 10))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);

    // A list key exposes index/value rows.
    connection
        .execute_query(Some(&database), &format!("RPUSH {list} a b c"))
        .await
        .unwrap();
    let page = connection
        .fetch_page(&database, &list, PageRequest::new(2, 2))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1, "page 2 of 3 entries with size 2");
    assert_eq!(page.total_rows, Some(3));

    // Rename and delete through the object actions.
    let renamed = format!("{prefix}:renamed");
    connection
        .rename_table(&database, &hash, &renamed)
        .await
        .unwrap();
    let tables = connection.list_tables(&database).await.unwrap();
    assert!(tables.iter().any(|table| table.name == renamed));
    connection.drop_table(&database, &renamed).await.unwrap();
    connection.drop_table(&database, &list).await.unwrap();

    // Editing a string value must not clear the key's TTL, on Redis 6+ (KEEPTTL) or older servers
    // (where the driver saves and restores the PTTL).
    let ttl_key = format!("{prefix}:ttl");
    connection
        .execute_query(Some(&database), &format!("SET {ttl_key} one EX 300"))
        .await
        .unwrap();
    connection
        .update_rows(
            &database,
            &ttl_key,
            &[RowUpdate {
                set: vec![("value".to_string(), Some("two".to_string()))],
                keys: vec![("value".to_string(), Some("one".to_string()))],
            }],
        )
        .await
        .unwrap();
    let ttl = connection
        .execute_query(Some(&database), &format!("TTL {ttl_key}"))
        .await
        .unwrap();
    match ttl.rows.first().and_then(|row| row.first()) {
        Some(rustgrid_core::CellValue::Int(ttl)) => {
            assert!(*ttl > 0 && *ttl <= 300, "TTL after edit was {ttl}");
        }
        other => panic!("unexpected TTL reply {other:?}"),
    }
    connection.drop_table(&database, &ttl_key).await.unwrap();

    // A read-only connection refuses writes but still reads.
    let mut read_only = config.clone();
    read_only.settings.read_only = true;
    let read_only = RedisDriver::new().connect(&read_only).await.unwrap();
    assert!(
        read_only
            .execute_query(Some(&database), "SET rustgrid:test:readonly 1")
            .await
            .is_err()
    );
    assert!(read_only.server_version().await.is_ok());

    connection.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires a live Redis (RUSTGRID_REDIS_*)"]
async fn pages_creates_and_expires_keys() {
    let config = config();
    let database = config.database.clone().unwrap_or_else(|| "0".to_string());
    let database = format!("db{database}");
    let connection = RedisDriver::new().connect(&config).await.expect("connect");
    let prefix = format!("rustgrid:page:{}", std::process::id());

    // 210 keys so the 200-key page leaves a cursor behind.
    let mut mset = String::from("MSET");
    for index in 0..210 {
        mset.push_str(&format!(" {prefix}:k{index:03} v{index}"));
    }
    connection
        .execute_query(Some(&database), &mset)
        .await
        .unwrap();

    let (first, cursor) = connection
        .list_objects_page(&database, None, None)
        .await
        .unwrap();
    assert!(first.len() >= 200, "first page: {}", first.len());
    let mut seen = first.len();
    let mut cursor = cursor;
    while let Some(current) = cursor.clone() {
        let (next, next_cursor) = connection
            .list_objects_page(&database, Some(&current), None)
            .await
            .unwrap();
        assert!(!next.is_empty(), "a non-final page always has keys");
        seen += next.len();
        cursor = next_cursor;
    }
    assert!(seen >= 210, "every key must be reachable through the pages");

    // A namespace-filtered page only carries the matching keys.
    let (namespaced, _) = connection
        .list_objects_page(&database, None, Some(&format!("{prefix}:*")))
        .await
        .unwrap();
    assert!(!namespaced.is_empty());
    assert!(
        namespaced
            .iter()
            .all(|table| table.name.starts_with(&prefix))
    );

    let namespaces = connection.list_namespaces(&database).await.unwrap();
    assert!(
        namespaces
            .iter()
            .any(|namespace| namespace.name == "rustgrid:" && namespace.objects >= 210),
        "namespaces: {namespaces:?}"
    );

    // Create one key of each type, with a TTL, then read the TTL back.
    let string_key = format!("{prefix}:created:str");
    connection
        .create_object(
            &database,
            &string_key,
            &ObjectCreate {
                kind: "string".to_string(),
                ttl: Some(120),
                values: vec![("value".to_string(), Some("hello".to_string()))],
            },
        )
        .await
        .unwrap();
    let ttl = connection.object_ttl(&database, &string_key).await.unwrap();
    assert!(
        ttl.is_some_and(|ttl| (1..=120).contains(&ttl)),
        "ttl {ttl:?}"
    );
    let page = connection
        .fetch_page(&database, &string_key, PageRequest::new(1, 1))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);

    let hash_key = format!("{prefix}:created:hash");
    connection
        .create_object(
            &database,
            &hash_key,
            &ObjectCreate {
                kind: "hash".to_string(),
                ttl: None,
                values: vec![
                    ("field".to_string(), Some("f".to_string())),
                    ("value".to_string(), Some("v".to_string())),
                ],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        connection.object_ttl(&database, &hash_key).await.unwrap(),
        Some(-1)
    );
    connection
        .set_object_ttl(&database, &hash_key, Some(60))
        .await
        .unwrap();
    let ttl = connection.object_ttl(&database, &hash_key).await.unwrap();
    assert!(ttl.is_some_and(|ttl| (0..=60).contains(&ttl)));
    connection
        .set_object_ttl(&database, &hash_key, None)
        .await
        .unwrap();
    assert_eq!(
        connection.object_ttl(&database, &hash_key).await.unwrap(),
        Some(-1)
    );

    // Pub/sub: a message published through the shared connection reaches the watch stream.
    let channel = format!("{prefix}:chan");
    let mut watch = connection
        .watch(
            Some(&database),
            &WatchSpec::Subscribe {
                channels: vec![channel.clone()],
                patterns: Vec::new(),
            },
        )
        .await
        .unwrap()
        .expect("pubsub watch");
    connection
        .execute_query(Some(&database), &format!("PUBLISH {channel} payload"))
        .await
        .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), watch.next())
        .await
        .expect("a pub/sub message within 5s")
        .unwrap()
        .expect("an event");
    assert_eq!(event.channel, channel);
    assert_eq!(event.text, "payload");

    // MONITOR: a command run afterwards shows up on the monitor stream.
    let mut monitor = connection
        .watch(Some(&database), &WatchSpec::Monitor)
        .await
        .unwrap()
        .expect("monitor watch");
    connection
        .execute_query(Some(&database), "PING")
        .await
        .unwrap();
    let mut seen = false;
    for _ in 0..5 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), monitor.next())
            .await
            .expect("a monitor line within 5s")
            .unwrap();
        let Some(event) = event else { break };
        if event.text.contains("PING") {
            seen = true;
            break;
        }
    }
    assert!(seen, "the monitor stream should show the PING");

    // Clean up.
    let keys = connection.list_tables(&database).await.unwrap();
    for table in keys {
        if table.name.starts_with(&prefix) {
            connection.drop_table(&database, &table.name).await.unwrap();
        }
    }
    connection.close().await.unwrap();
}
