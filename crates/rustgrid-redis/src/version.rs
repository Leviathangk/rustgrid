//! Server version parsing and the feature gates that depend on it.
//!
//! A single build talks to servers from Redis 2.8 to 7.x, plus Redis-compatible implementations
//! (KeyDB, Valkey, Dragonfly, Microsoft Garnet). Commands introduced by a later release are only
//! sent when the server reports at least that version.

/// A parsed `redis_version` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServerVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ServerVersion {
    /// The oldest version this driver's feature gates distinguish (Redis 2.8).
    pub const UNKNOWN: Self = Self {
        major: 0,
        minor: 0,
        patch: 0,
    };

    /// Parses `7.2.4`, `6.0`, `255.255.255`, ignoring an optional trailing suffix.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let core = text
            .split(|character: char| !character.is_ascii_digit() && character != '.')
            .next()
            .unwrap_or_default();
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Whether the server is at least `major.minor`.
    pub fn at_least(self, major: u16, minor: u16) -> bool {
        (self.major, self.minor) >= (major, minor)
    }

    /// `MEMORY USAGE` and `UNLINK` were added in 4.0.
    pub fn has_memory_usage(self) -> bool {
        self.at_least(4, 0)
    }

    /// `UNLINK` (asynchronous delete) was added in 4.0; older servers use `DEL`.
    pub fn has_unlink(self) -> bool {
        self.at_least(4, 0)
    }

    /// Streams (`XADD`, `XRANGE`, `XINFO`, …) were added in 5.0.
    pub fn has_streams(self) -> bool {
        self.at_least(5, 0)
    }

    /// `SET … KEEPTTL` was added in 6.0.
    pub fn has_keep_ttl(self) -> bool {
        self.at_least(6, 0)
    }

    /// `ACL` (Redis 6 users) was added in 6.0.
    pub fn has_acl(self) -> bool {
        self.at_least(6, 0)
    }
}

impl std::fmt::Display for ServerVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The server identity and version read from `INFO server`.
#[derive(Debug, Clone)]
pub(crate) struct ServerInfo {
    pub version: ServerVersion,
    /// The raw version string (`redis_version`, `valkey_version`, …).
    pub version_text: String,
    /// The server software as reported (`redis`, `valkey`, `keydb`, `garnet`, `dragonfly`, …).
    pub flavor: String,
}

impl ServerInfo {
    /// Parses the `INFO server` payload.
    ///
    /// Redis reports `redis_version`; Valkey also reports `valkey_version` and prefers it. Garnet
    /// and Dragonfly report `redis_version` for compatibility, so a missing flavour falls back to
    /// `redis`.
    pub fn parse(info: &str) -> Self {
        let mut version_text = String::new();
        let mut flavor = String::new();
        for line in info.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "valkey_version" => {
                    version_text = value.to_string();
                    flavor = "valkey".to_string();
                }
                "redis_version" if version_text.is_empty() => {
                    version_text = value.to_string();
                }
                "keydb_version" => {
                    version_text = value.to_string();
                    flavor = "keydb".to_string();
                }
                "dragonfly_version" => {
                    version_text = value.to_string();
                    flavor = "dragonfly".to_string();
                }
                "garnet_version" => {
                    version_text = value.to_string();
                    flavor = "garnet".to_string();
                }
                _ => {}
            }
        }
        if flavor.is_empty() {
            flavor = "redis".to_string();
        }
        let version = ServerVersion::parse(&version_text).unwrap_or(ServerVersion::UNKNOWN);
        Self {
            version,
            version_text,
            flavor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_version_strings() {
        assert_eq!(
            ServerVersion::parse("7.2.4"),
            Some(ServerVersion {
                major: 7,
                minor: 2,
                patch: 4
            })
        );
        assert_eq!(
            ServerVersion::parse("6.0"),
            Some(ServerVersion {
                major: 6,
                minor: 0,
                patch: 0
            })
        );
        assert_eq!(
            ServerVersion::parse("255.255.255"),
            Some(ServerVersion {
                major: 255,
                minor: 255,
                patch: 255
            })
        );
        assert_eq!(ServerVersion::parse("not-a-version"), None);
    }

    #[test]
    fn gates_match_the_release_that_introduced_the_command() {
        let old = ServerVersion {
            major: 3,
            minor: 2,
            patch: 0,
        };
        assert!(!old.has_memory_usage());
        assert!(!old.has_streams());
        assert!(!old.has_keep_ttl());

        let modern = ServerVersion {
            major: 7,
            minor: 0,
            patch: 0,
        };
        assert!(modern.has_unlink());
        assert!(modern.has_streams());
        assert!(modern.has_keep_ttl());
        assert!(modern.has_acl());
    }

    #[test]
    fn reads_the_version_and_flavour_from_info() {
        let info =
            ServerInfo::parse("# Server\r\nredis_version:7.2.4\r\nredis_mode:standalone\r\n");
        assert_eq!(info.version_text, "7.2.4");
        assert_eq!(info.flavor, "redis");
        assert_eq!(info.version, ServerVersion::parse("7.2.4").unwrap());

        let valkey = ServerInfo::parse("redis_version:7.2.4\r\nvalkey_version:8.0.1\r\n");
        assert_eq!(valkey.version_text, "8.0.1");
        assert_eq!(valkey.flavor, "valkey");
    }
}
