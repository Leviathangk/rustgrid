# Redis driver

`crates/rustgrid-redis` adds Redis (and Redis-compatible servers) to RustGrid through the same
`Driver`/`Connection` abstraction as the SQL engines. It is built on **redis-rs 0.32** with only the
`tokio-comp`, `cluster-async`, `sentinel` and `tokio-rustls-comp` features, so it speaks Redis
standalone, Sentinel and Cluster, with rustls TLS and no OpenSSL.

## How Redis maps onto the object model

Redis has no tables. The driver maps:

| RustGrid concept | Redis |
| --- | --- |
| database | `db0` … `dbN` (a numbered keyspace) |
| table | a key |
| table row | one entry of the key's value |
| column | the key type's fields |

The row/column shape is chosen from `TYPE`:

| `TYPE` | Columns | Read | Write |
| --- | --- | --- | --- |
| `string` | `value` | `GET` | `SET` (`KEEPTTL` from 6.0; PTTL is saved/restored on older servers) |
| `list` | `index`, `value` | `LRANGE` | `RPUSH`, `LSET`; deleting an index uses a marker + `LREM` |
| `set` | `member` | `SMEMBERS` | `SADD`, `SREM` (renaming = `SREM` + `SADD`) |
| `zset` | `member`, `score` | `ZRANGE … WITHSCORES` | `ZADD`, `ZREM` |
| `hash` | `field`, `value` | `HGETALL` | `HSET`, `HDEL` (renaming = `HDEL` + `HSET`) |
| `stream` | `id`, `field`, `value` | `XRANGE`, `XLEN` | read-only (entries are append-only) |
| `ReJSON-RL` / `json` | `value` | `JSON.GET` | read-only (use the console: `JSON.SET`) |
| anything else | — | not shown | — |

The identity column (`field`, `member`, `index`, `id`, `value`) is marked as the primary key, which is
what the shared grid uses to build the `UPDATE`/`DELETE` keys.

Lists and sorted sets page natively (`LRANGE`/`ZRANGE`); hashes and sets have no offset, so a page is
cut from the full value after sorting, and a value larger than 200 000 entries is refused with a
message pointing at `HSCAN`/`SSCAN` in a query tab.

## Connection modes

`redis.mode` (connection form → 连接类型):

- **Standalone** — `host:port`, optional ACL username/password, database `0..15`.
- **Sentinel** — `redis.sentinel.master` (master name) plus `redis.sentinel.hosts`
  (`host:port,host:port`) and an optional `redis.sentinel.password`; the master is discovered through
  `SENTINEL` and the normal username/password apply to the Redis node.
- **Cluster** — seed nodes from `redis.cluster_nodes` (blank falls back to host/port). Only `db0`
  exists; `CLUSTER SLOTS` drives both routing (redis-rs) and the per-master `SCAN`.

Every database gets its own multiplexed connection, because the selected database is connection state
and tabs share the connection. A **tunnel** is only accepted for standalone servers: Sentinel and
Cluster return addresses the single local forwarder cannot reach.

TLS modes map onto Redis like this: `Disabled` = plaintext, `Preferred` = TLS with a plaintext retry,
`Required` = TLS without certificate verification, `VerifyCa`/`VerifyIdentity` = rustls verification
(the CA/client certificate files from the TLS page are used). Certificate verification and a tunnel
cannot be combined, because the certificate is issued for the server, not for `127.0.0.1`.

## Version differences

The server version is read from `INFO server` (`redis_version`, `valkey_version`, `keydb_version`, …)
once per connection, and gates the commands a later release introduced:

| Feature | Needs |
| --- | --- |
| `MEMORY USAGE`, `UNLINK` | 4.0 |
| streams (`XLEN`, `XRANGE`) | 5.0 (a stream key cannot exist before that) |
| `SET … KEEPTTL`, `ACL` | 6.0 |
| `EVAL`/`FCALL` | 2.6 / 7.0 (allowed, not gated) |

RESP2 is used deliberately: it works from Redis 2.8 through 7.x, while RESP3 requires `HELLO` (6.0).
`CLIENT SETINFO` failures on older servers are ignored by redis-rs. Authentication retries without
the username when a pre-6 server rejects the two-argument `AUTH`, which is redis-rs' own fallback.

## Query editor

A query tab runs **one command per line**, with redis-cli quoting (`"…"`, `'…'`, backslash escapes).
`split_statements` uses the same splitter, so Run SQL File works on a command list.

Commands that would corrupt the shared connection are refused with an explanation:

- connection state: `SELECT`, `AUTH`, `HELLO`, `RESET`, `QUIT`, `READONLY`, `READWRITE`;
- transactions: `MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH` (the connection is shared by tabs);
- `CLIENT REPLY/TRACKING/CACHING/PAUSE/…`;
- blocking commands (`BLPOP`, `SUBSCRIBE`, `MONITOR`, `WAIT`, …);
- `SHUTDOWN`, `DEBUG`, `FAILOVER`, `REPLICAOF`, `SWAPDB`.

`只读安全模式` (the Advanced page's read-only toggle) rejects every write command, including module
writes such as `JSON.SET`, and every grid edit.

## Key management and namespaces

The object list is the key list. For key-value engines (`DriverCapability::KeyValue`) its detailed
layout is # / 键名 / 类型 / TTL / 内存 and it is the **only** layout for a key list (the 详细列表 /
平铺网格 switch is not shown), the tree under a database shows the **namespace folders** (with key
counts) instead of SQL categories, and the toolbar adds **新建键 / 设置 TTL / Redis 工具**. The key
list shows **加载更多** while the last `SCAN` page left a cursor. The Redis tools window's 键 pane
carries the same operations plus a namespace picker:

- **new key**: name, type (`string`/`hash`/`list`/`set`/`zset`/`stream`), field/member, value/score
  and an optional TTL;
- **TTL**: `EXPIRE` (or `PERSIST` for 永久), with the key name prefilled by 设置 TTL from the object
  list;
- **namespaces**: `list_namespaces` groups the scanned keys by their first `:` prefix and shows the
  counts; picking one in the tree (or the tools window) applies `prefix*` as the `SCAN MATCH`
  pattern (`object_pattern`) and reloads the object list, picking 全部 clears it.

Opening a key shows the normal grid with its columns chosen from `TYPE`, prefixed by a key header
with the type badge, key name, type, TTL and memory. **Hash and set keys open on a dedicated key
page** (`app/grid_key.rs`) instead of the SQL grid: the page has a key header bar with 添加字段 /
添加成员, 删除选中, 保存修改 and 刷新, a numbered Field/Value or Member table (the cells edit in
place like the grid), and a compact pager. There is no WHERE/sort/field-layout toolbar or record
toolbar on that page.

The object list holds at most `redis.key_limit` keys (default 1000) per *page load*; each 加载更多
reads the next `SCAN` page (200 keys). Cluster mode has no cross-node cursor, so a cluster lists its
bounded snapshot in one go.

## Redis tools window

An engine-agnostic OS window opened from the object toolbar (`redis_tools.rs`), showing:

| Section | What it does |
| --- | --- |
| 键 | new key, TTL, namespace picker (see above) |
| 发布订阅 | `SUBSCRIBE`/`PSUBSCRIBE` through `Connection::watch`, a message log and `PUBLISH` |
| 命令监控 | `MONITOR` through `Connection::watch`, start/stop, live log |
| 慢日志 | `SLOWLOG GET 100` rendered as one line per entry |
| ACL 用户 | `ACL LIST`, select a user to fill the rule text, `ACL SETUSER` / `ACL DELUSER` |

The live streams run on the tokio runtime and forward events through a channel to a gpui task; a
generation counter drops events from a stopped stream. `MONITOR` on a cluster follows the first
master (the stream is per node). The ACL section needs Redis 6+ and an account allowed to run `ACL`;
on an older server the error is shown in the window's status line.

## Limitations

- The object list still labels its container 表: the shared tab/noun vocabulary has no per-driver
  key label yet.
- There is no separate key-detail tab: a key opens in the normal grid, whose columns depend on
  `TYPE`, with a key header above it.
- The Users main tab stays disabled (`supports_users` is `false`); Redis ACL is managed in the tools
  window instead.
- Backup/Restore (`Backup` tab) is disabled: the SQL dump pipeline does not apply to keys.
- Keys of module types other than ReJSON are listed but not opened.

## Tests

Unit tests cover version parsing/gates, the command splitter/quoting, write/blocking classification,
the type → column mapping and reply rendering:

```text
cargo test -p rustgrid-redis
```

The live test is ignored by default:

```text
docker run -d --name rg-redis -p 6379:6379 redis:7
RUSTGRID_REDIS_PASSWORD= cargo test -p rustgrid-redis -- --ignored
```

It writes keys under a `rustgrid:test:<pid>` prefix and removes them again.
