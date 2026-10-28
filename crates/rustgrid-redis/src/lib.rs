//! The Redis driver: a client for standalone Redis (and Redis-compatible servers), Redis Sentinel
//! deployments and Redis Cluster.
//!
//! Redis has no tables, so this driver maps its data model onto RustGrid's object model: a database
//! (`db0` … `dbN`) is the object container, a key is an "object", and opening a key shows its
//! content as rows (a hash's fields, a list's elements, a stream's entries, …). Everything else —
//! the grid, filters, export, the query editor — is the shared UI, driven through
//! [`rustgrid_core::Connection`].
//!
//! Feature gating follows the server version read at connect time, because the same build talks to
//! Redis 2.8 through 7.x (and Redis-compatible servers such as KeyDB, Valkey, Dragonfly and
//! Garnet): `MEMORY USAGE`/`UNLINK` need 4.0, streams need 5.0, `SET … KEEPTTL` needs 6.0.

mod command;
mod connection;
mod driver;
mod keys;
mod reply;
mod version;

pub use connection::RedisConnection;
pub use driver::{
    OPT_CLUSTER_NODES, OPT_KEY_LIMIT, OPT_MODE, OPT_SENTINEL_HOSTS, OPT_SENTINEL_MASTER,
    OPT_SENTINEL_PASSWORD, RedisDriver,
};
pub use keys::KeyType;
pub use version::ServerVersion;
