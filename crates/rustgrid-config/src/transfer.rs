//! Connection import/export.
//!
//! Three file shapes are supported, all engine-agnostic (a connection is a driver id plus plain
//! fields, exactly like [`ConnectionProfile`]):
//!
//! * **RustGrid** (`*.rgc`) — this app's own bundle: pretty-printed JSON with a `format`/`version`
//!   header. Passwords may be omitted (the export dialog's 导出密码 check box).
//! * **Navicat** (`*.ncx`) — Navicat's XML export. A connection is one `<Connection …/>` element
//!   whose attributes carry the fields; the password is hex-encoded AES-128-CBC (the
//!   `libcckeylibcckey` key / `libcciv libcciv ` IV scheme), which is what other tools such as DBX
//!   decrypt.
//! * **DBX** — DBX's plain JSON export: either a bare array of connections or an object with a
//!   `connections` array (DBX's own `format: "dbx-config"` bundle). Encrypted DBX exports are not
//!   supported and are rejected when their JSON does not carry the expected shape.
//!
//! `TransferConnection` is the common DTO the app turns into [`ConnectionProfile`]s; keeping the
//! parsers here means a new external format is one function, with no UI changes.

use std::collections::BTreeMap;

use aes::Aes128;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use chacha20poly1305::aead::{Aead, Generate, KeyInit as _};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rustgrid_core::{
    ConnectionOptions, ConnectionProfile, TlsMode, TunnelAuth, TunnelKind, TunnelLayer,
};
use serde::{Deserialize, Serialize};

use crate::GroupEntry;

/// The `format` marker written into a RustGrid `.rgc` bundle.
pub const RUSTGRID_FORMAT: &str = "rustgrid-connections";
/// The RustGrid bundle schema version.
pub const RUSTGRID_VERSION: u32 = 1;
/// The default file name offered by the export dialog.
pub const RUSTGRID_FILE_NAME: &str = "rustgrid-connections.rgc";

/// Navicat 12+'s password key/IV (AES-128-CBC), the scheme DBX and other importers use.
const NAVICAT_KEY: &[u8; 16] = b"libcckeylibcckey";
const NAVICAT_IV: &[u8; 16] = b"libcciv libcciv ";
/// Navicat 11's Blowfish password key seed; its key is `SHA1` of this string.
const NAVICAT_V1_KEY_SEED: &[u8] = b"3DC5CA39";

/// One connection as read from (or written to) a transfer file. Engine-agnostic: `driver` is a
/// [`DriverId`](rustgrid_core::DriverId) string and every other field maps 1:1 onto a
/// [`ConnectionProfile`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferConnection {
    /// The source profile id. Only a RustGrid `.rgc` carries it; it lets the importer rebuild the
    /// connection tree's groups and ordering. Absent for Navicat/DBX.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub driver: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
    #[serde(
        default,
        skip_serializing_if = "is_zero",
        deserialize_with = "port_from_any"
    )]
    pub port: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "ConnectionOptions::is_default")]
    pub settings: ConnectionOptions,
    /// The plaintext password, or the protected (encrypted) token as read from a file. Absent when
    /// the export left it out or the source could not be decrypted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// The optional environment colour id (`red`/`yellow`/`green`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// How a RustGrid bundle protects the passwords it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordProtection {
    /// Passwords are absent or already plaintext (a legacy/hand-written file).
    #[default]
    None,
    /// Passwords are XChaCha20-Poly1305-encrypted with the app's built-in key.
    Builtin,
}

/// A RustGrid `.rgc` bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub password_protection: PasswordProtection,
    /// The connection tree layout restricted to the exported connections (groups and their
    /// ordering). Empty for a flat bundle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupEntry>,
    #[serde(default)]
    pub connections: Vec<TransferConnection>,
}

/// The external format a file is read as, chosen from the 导入连接 submenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    RustGrid,
    Navicat,
    Dbx,
}

/// The result of parsing a transfer file: the connections that map onto a supported engine, the
/// tree layout they came with, and how many entries were skipped (their engine is not one this app
/// implements) or had a password that could not be decrypted.
#[derive(Debug, Clone, Default)]
pub struct ParsedConnections {
    pub connections: Vec<TransferConnection>,
    pub groups: Vec<GroupEntry>,
    pub skipped: usize,
    pub passwords_failed: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("could not parse the connection file: {0}")]
    Parse(String),
}

/// Parse `content` as `format`.
pub fn parse(content: &str, format: SourceFormat) -> Result<ParsedConnections, TransferError> {
    match format {
        SourceFormat::RustGrid => parse_rustgrid(content),
        SourceFormat::Navicat => parse_navicat(content),
        SourceFormat::Dbx => parse_dbx(content),
    }
}

/// Build a RustGrid bundle from connections already prepared for export, plus the tree layout that
/// ties them together (groups and ordering).
pub fn build_rustgrid_file(
    connections: Vec<TransferConnection>,
    groups: Vec<GroupEntry>,
) -> ConnectionsFile {
    ConnectionsFile {
        format: RUSTGRID_FORMAT.to_string(),
        version: RUSTGRID_VERSION,
        password_protection: PasswordProtection::None,
        groups,
        connections,
    }
}

/// Serialize a RustGrid bundle as pretty JSON, protecting any password it carries. The passwords
/// are encrypted (XChaCha20-Poly1305, [`PASSWORD_KEY`]) and the file is flagged so the importer
/// knows to decrypt them; a bundle with no password is written as-is.
pub fn serialize_rustgrid(file: &ConnectionsFile) -> Result<String, TransferError> {
    let mut file = file.clone();
    let mut encrypted = false;
    for connection in &mut file.connections {
        if let Some(password) = connection.password.take()
            && !password.is_empty()
        {
            connection.password = Some(encrypt_password(&password)?);
            encrypted = true;
        }
    }
    if encrypted {
        file.password_protection = PasswordProtection::Builtin;
    }
    serde_json::to_string_pretty(&file).map_err(|error| TransferError::Parse(error.to_string()))
}

/// Build a [`ConnectionProfile`] from a transfer entry, generating a fresh id. `name` is the
/// already-deduplicated name the caller chose.
pub fn to_profile(connection: &TransferConnection, id: String, name: String) -> ConnectionProfile {
    ConnectionProfile {
        id,
        name,
        driver: rustgrid_core::DriverId::new(connection.driver.clone()),
        host: connection.host.clone(),
        port: connection.port,
        username: connection.username.clone(),
        database: connection.database.clone(),
        options: connection.options.clone(),
        settings: connection.settings.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// RustGrid `.rgc`
// ---------------------------------------------------------------------------------------------

fn parse_rustgrid(content: &str) -> Result<ParsedConnections, TransferError> {
    let mut file: ConnectionsFile =
        serde_json::from_str(content).map_err(|error| TransferError::Parse(error.to_string()))?;
    if file.format != RUSTGRID_FORMAT {
        return Err(TransferError::Parse(format!(
            "unexpected format \"{}\"",
            file.format
        )));
    }
    let mut passwords_failed = 0;
    if file.password_protection == PasswordProtection::Builtin {
        for connection in &mut file.connections {
            if let Some(token) = connection.password.take() {
                match decrypt_password(&token) {
                    Some(password) => connection.password = Some(password),
                    None => passwords_failed += 1,
                }
            }
        }
    }
    Ok(ParsedConnections {
        connections: file.connections,
        groups: file.groups,
        skipped: 0,
        passwords_failed,
    })
}

// ---------------------------------------------------------------------------------------------
// Navicat `.ncx`
// ---------------------------------------------------------------------------------------------

fn parse_navicat(content: &str) -> Result<ParsedConnections, TransferError> {
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut parsed = ParsedConnections::default();
    loop {
        match reader.read_event() {
            Err(error) => return Err(TransferError::Parse(format!("invalid XML: {error}"))),
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(quick_xml::events::Event::Start(element))
            | Ok(quick_xml::events::Event::Empty(element)) => {
                if !element.name().as_ref().eq_ignore_ascii_case(b"Connection") {
                    continue;
                }
                let mut values: BTreeMap<String, String> = BTreeMap::new();
                for attribute in element.attributes().flatten() {
                    let key = normalize_key(&String::from_utf8_lossy(attribute.key.as_ref()));
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map(|value| value.into_owned())
                        .unwrap_or_default();
                    values.insert(key, value);
                }
                match navicat_connection(&values) {
                    Some((connection, password_failed)) => {
                        if password_failed {
                            parsed.passwords_failed += 1;
                        }
                        parsed.connections.push(connection);
                    }
                    None => parsed.skipped += 1,
                }
            }
            _ => {}
        }
    }
    Ok(parsed)
}

/// Map one Navicat `<Connection>`'s normalized attributes onto a transfer entry plus whether a
/// password was present but could not be decrypted. Returns `None` when the engine is not one this
/// app supports.
fn navicat_connection(values: &BTreeMap<String, String>) -> Option<(TransferConnection, bool)> {
    let raw_type = pick(
        values,
        &[
            "conntype",
            "databasetype",
            "driver",
            "dbtype",
            "connectiontype",
            "type",
        ],
    )
    .unwrap_or_default();
    let port = pick(values, &["port", "serverport"])
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0);
    let driver = navicat_driver(&raw_type, port)?;

    let name = pick(
        values,
        &["connectionname", "name", "connname", "caption", "title"],
    )
    .filter(|name| !name.trim().is_empty())
    .unwrap_or_else(|| driver.to_string());

    let mut options: BTreeMap<String, String> = BTreeMap::new();
    let mut settings = ConnectionOptions::default();

    let host;
    let username;
    let mut database;
    if driver == "sqlite" {
        // A file engine stores its path in the profile's database field.
        let file = pick(
            values,
            &[
                "databasefile",
                "databasefilename",
                "filename",
                "filepath",
                "databasepath",
                "dbfile",
                "sqlitepath",
                "path",
                "database",
            ],
        )
        .unwrap_or_default();
        host = String::new();
        username = String::new();
        database = Some(file);
    } else {
        host = pick(
            values,
            &["host", "server", "hostname", "serverhost", "address"],
        )
        .unwrap_or_default();
        username = pick(values, &["username", "user", "uid"]).unwrap_or_default();
        database = pick(
            values,
            &[
                "database",
                "databasename",
                "initialdatabase",
                "servicename",
                "schema",
            ],
        )
        .filter(|value| !value.is_empty());
        if driver == "oracle" {
            if let Some(sid) = pick(values, &["sid"]).filter(|value| !value.is_empty()) {
                database = Some(sid);
                options.insert("oracle.connect_type".to_string(), "sid".to_string());
            } else {
                options.insert("oracle.connect_type".to_string(), "service".to_string());
            }
        }
    }

    let raw_password = pick(values, &["password"]).unwrap_or_default();
    let password = decrypt_navicat_password(&raw_password).filter(|password| !password.is_empty());
    let password_failed = !raw_password.trim().is_empty() && password.is_none();

    if truthy(pick(values, &["ssl", "usessl"]).as_deref()) {
        settings.tls.mode = TlsMode::Preferred;
    }
    if let Some(layer) = navicat_tunnel(values) {
        settings.tunnel.push(layer);
    }

    Some((
        TransferConnection {
            id: None,
            name,
            driver: driver.to_string(),
            host,
            port: port.unwrap_or(0),
            username,
            database,
            options,
            settings,
            password,
            color: None,
        },
        password_failed,
    ))
}

fn navicat_driver(raw_type: &str, port: Option<u16>) -> Option<&'static str> {
    let key = normalize_key(raw_type);
    match key.as_str() {
        "1" => Some("mysql"),
        "2" => Some("postgresql"),
        "3" => Some("sqlite"),
        "4" => Some("oracle"),
        "5" => Some("mariadb"),
        "7" => Some("sqlserver"),
        "" => None,
        _ => {
            if key.contains("mariadb") {
                Some("mariadb")
            } else if key.contains("mysql") {
                Some("mysql")
            } else if key.contains("postgres") {
                Some("postgresql")
            } else if key.contains("sqlite") {
                Some("sqlite")
            } else if key.contains("sqlserver") || key.contains("mssql") {
                Some("sqlserver")
            } else if key.contains("oracle") {
                Some("oracle")
            } else if key.contains("odbc") {
                Some("odbc")
            } else {
                port.and_then(|port| match port {
                    3306 => Some("mysql"),
                    5432 => Some("postgresql"),
                    1433 => Some("sqlserver"),
                    1521 => Some("oracle"),
                    _ => None,
                })
            }
        }
    }
}

fn navicat_tunnel(values: &BTreeMap<String, String>) -> Option<TunnelLayer> {
    let enabled = truthy(
        pick(
            values,
            &[
                "ssh",
                "usessh",
                "sshenabled",
                "enablessh",
                "usesshtunnel",
                "sshtunnelenabled",
            ],
        )
        .as_deref(),
    );
    if !enabled {
        return None;
    }
    let host = pick(values, &["sshhost", "sshtunnelhost", "tunnelhost"])?;
    let username = pick(
        values,
        &[
            "sshusername",
            "sshuser",
            "sshtunnelusername",
            "sshtunneluser",
            "tunnelusername",
        ],
    )?;
    if host.is_empty() || username.is_empty() {
        return None;
    }
    let auth_value = normalize_key(
        &pick(
            values,
            &[
                "sshauthenmethod",
                "sshauthmethod",
                "sshauthenticationmethod",
                "sshauthentication",
                "sshauth",
            ],
        )
        .unwrap_or_default(),
    );
    let key_path = pick(
        values,
        &[
            "sshprivatekey",
            "sshkeyfile",
            "sshkeypath",
            "sshidentityfile",
            "sshtunnelprivatekey",
        ],
    )
    .unwrap_or_default();
    let uses_key =
        auth_value.contains("key") || (!auth_value.contains("password") && !key_path.is_empty());
    let password = if uses_key {
        String::new()
    } else {
        pick(values, &["sshpassword", "sshtunnelpassword"])
            .as_deref()
            .and_then(decrypt_navicat_password)
            .unwrap_or_default()
    };
    let port = pick(values, &["sshport", "sshtunnelport", "tunnelport"])
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(22);
    Some(TunnelLayer {
        kind: TunnelKind::Ssh,
        host,
        port,
        username,
        password,
        key_path: if uses_key { key_path } else { String::new() },
        auth: if uses_key {
            TunnelAuth::KeyFile
        } else {
            TunnelAuth::Password
        },
    })
}

/// Decrypt a Navicat `.ncx` password field. Navicat 11 and 12+ use different ciphers, so both are
/// attempted: v2 (AES-128-CBC, Navicat 12+, where the hex is always a whole number of 16-byte
/// blocks) first, then v1 (Blowfish, Navicat 11, where the ciphertext length equals the password
/// length). A result that is not valid UTF-8 or contains a control character is rejected, so a
/// wrong cipher choice does not silently yield garbage. Returns `None` for empty, non-hex or
/// undecryptable input.
fn decrypt_navicat_password(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let ciphertext = hex::decode(trimmed).ok()?;
    if ciphertext.is_empty() {
        return None;
    }
    if ciphertext.len() % 16 == 0
        && let Some(password) = decrypt_navicat_v2(&ciphertext)
        && plausible_password(&password)
    {
        return Some(password);
    }
    decrypt_navicat_v1(&ciphertext).filter(|password| plausible_password(password))
}

/// Navicat 12+: AES-128-CBC with the fixed `libcckeylibcckey` key / `libcciv libcciv ` IV.
fn decrypt_navicat_v2(ciphertext: &[u8]) -> Option<String> {
    let plaintext = cbc::Decryptor::<Aes128>::new(NAVICAT_KEY.into(), NAVICAT_IV.into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .ok()?;
    String::from_utf8(plaintext).ok()
}

/// Navicat 11: Blowfish in a CFB-like chaining over ECB blocks. The key is `SHA1("3DC5CA39")` and
/// the IV is the encryption of an all-`0xFF` block (`D9 C7 C3 C8 87 0D 64 BD`).
fn decrypt_navicat_v1(ciphertext: &[u8]) -> Option<String> {
    use blowfish::Blowfish;
    use blowfish::cipher::{Block, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
    use sha1::Digest as _;

    let mut hasher = sha1::Sha1::new();
    hasher.update(NAVICAT_V1_KEY_SEED);
    let key = hasher.finalize();
    let cipher: Blowfish = Blowfish::new_from_slice(&key).ok()?;

    let block = |value: [u8; 8], decrypt: bool| -> [u8; 8] {
        let mut bytes = Block::<Blowfish>::from(value);
        if decrypt {
            cipher.decrypt_block(&mut bytes);
        } else {
            cipher.encrypt_block(&mut bytes);
        }
        let mut out = [0u8; 8];
        out.copy_from_slice(bytes.as_slice());
        out
    };

    let mut current = block([0xff; 8], false);
    let rounds = ciphertext.len() / 8;
    let leftover = ciphertext.len() % 8;
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    for offset in (0..rounds * 8).step_by(8) {
        let chunk = &ciphertext[offset..offset + 8];
        let mut decrypted = block(chunk.try_into().ok()?, true);
        for (index, byte) in chunk.iter().enumerate() {
            decrypted[index] ^= current[index];
            current[index] ^= byte;
        }
        plaintext.extend_from_slice(&decrypted);
    }
    if leftover > 0 {
        current = block(current, false);
        for (index, byte) in ciphertext[rounds * 8..].iter().enumerate() {
            plaintext.push(byte ^ current[index]);
        }
    }
    String::from_utf8(plaintext).ok()
}

/// Whether a decrypted password looks like real text. Invalid UTF-8 is already rejected by
/// [`String::from_utf8`]; this additionally rejects control characters, which a mis-detected cipher
/// (e.g. AES run over a Blowfish ciphertext) would otherwise produce.
fn plausible_password(password: &str) -> bool {
    !password.is_empty() && !password.chars().any(char::is_control)
}

// ---------------------------------------------------------------------------------------------
// Password protection for the exported `.rgc` file
// ---------------------------------------------------------------------------------------------

/// A fixed key for the exported file's passwords. This is deliberately **not** a secret: it only
/// keeps passwords out of plain sight when a `.rgc` file is shared, the same idea as Navicat and
/// DBX encrypting the password field of their exports. It is not a substitute for a passphrase.
const PASSWORD_KEY: [u8; 32] = [
    0x72, 0x75, 0x73, 0x74, 0x67, 0x72, 0x69, 0x64, 0x2d, 0x63, 0x6f, 0x6e, 0x6e, 0x65, 0x63, 0x74,
    0x69, 0x6f, 0x6e, 0x73, 0x2d, 0x76, 0x31, 0x2d, 0x78, 0x63, 0x68, 0x61, 0x32, 0x30, 0x70, 0x79,
];
/// XChaCha20-Poly1305 nonce length.
const NONCE_LEN: usize = 24;

fn password_cipher() -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new_from_slice(&PASSWORD_KEY).expect("the export key is a valid 32-byte key")
}

/// Encrypt one password for the exported file: `base64(nonce || ciphertext || tag)`.
fn encrypt_password(plaintext: &str) -> Result<String, TransferError> {
    let nonce = XNonce::generate();
    let ciphertext = password_cipher()
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| TransferError::Parse("could not encrypt a password".to_string()))?;
    let mut blob = nonce.as_slice().to_vec();
    blob.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(blob))
}

/// Decrypt one password from an exported file; `None` when it is not a valid protected value.
fn decrypt_password(encoded: &str) -> Option<String> {
    let blob = STANDARD.decode(encoded).ok()?;
    if blob.len() <= NONCE_LEN {
        return None;
    }
    let nonce = XNonce::try_from(&blob[..NONCE_LEN]).ok()?;
    let plaintext = password_cipher().decrypt(&nonce, &blob[NONCE_LEN..]).ok()?;
    String::from_utf8(plaintext).ok()
}

// ---------------------------------------------------------------------------------------------
// DBX
// ---------------------------------------------------------------------------------------------

fn parse_dbx(content: &str) -> Result<ParsedConnections, TransferError> {
    let value: serde_json::Value =
        serde_json::from_str(content).map_err(|error| TransferError::Parse(error.to_string()))?;
    let array = match &value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(object) => object
            .get("connections")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                TransferError::Parse("no connections array (an encrypted export?)".to_string())
            })?,
        _ => {
            return Err(TransferError::Parse(
                "expected a connections array or object".to_string(),
            ));
        }
    };

    let mut parsed = ParsedConnections::default();
    for item in array {
        match dbx_connection(item) {
            Some(connection) => parsed.connections.push(connection),
            None => parsed.skipped += 1,
        }
    }
    Ok(parsed)
}

fn dbx_connection(value: &serde_json::Value) -> Option<TransferConnection> {
    let object = value.as_object()?;
    let db_type = get_str(object, "db_type").unwrap_or_default();
    let driver_profile = get_str(object, "driver_profile");
    let driver = dbx_driver(&db_type, driver_profile.as_deref())?;

    let name = get_str(object, "name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| driver.to_string());
    let mut host = get_str(object, "host").unwrap_or_default();
    let port = object.get("port").and_then(json_u16).unwrap_or(0);
    let username = get_str(object, "username").unwrap_or_default();
    let mut database = get_str(object, "database").filter(|value| !value.is_empty());

    let mut options: BTreeMap<String, String> = BTreeMap::new();
    let mut settings = ConnectionOptions::default();

    if driver == "sqlite" {
        // DBX keeps a SQLite file path in `host`; the profile stores it as the database.
        database = Some(host).filter(|value| !value.is_empty());
        host = String::new();
    }
    if driver == "oracle" {
        let connect_type = match get_str(object, "oracle_connection_type").as_deref() {
            Some(value) if value.eq_ignore_ascii_case("sid") => "sid",
            _ => "service",
        };
        options.insert("oracle.connect_type".to_string(), connect_type.to_string());
    }
    if driver == "odbc"
        && let Some(connection_string) =
            get_str(object, "connection_string").filter(|value| !value.is_empty())
    {
        options.insert("odbc.connection_string".to_string(), connection_string);
    }
    if object.get("ssl").and_then(serde_json::Value::as_bool) == Some(true) {
        settings.tls.mode = TlsMode::Preferred;
    }
    if let Some(layer) = dbx_tunnel(object.get("transport_layers")) {
        settings.tunnel.push(layer);
    }

    Some(TransferConnection {
        id: None,
        name,
        driver: driver.to_string(),
        host,
        port,
        username,
        database,
        options,
        settings,
        password: get_str(object, "password").filter(|value| !value.is_empty()),
        color: get_str(object, "color").filter(|value| !value.is_empty()),
    })
}

fn dbx_driver(db_type: &str, driver_profile: Option<&str>) -> Option<&'static str> {
    let key = driver_profile
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(db_type)
        .to_ascii_lowercase();
    match key.as_str() {
        "mariadb" => Some("mariadb"),
        "mysql" => Some("mysql"),
        "postgres" | "postgresql" => Some("postgresql"),
        "sqlite" => Some("sqlite"),
        "sqlserver" | "mssql" => Some("sqlserver"),
        "oracle" => Some("oracle"),
        "odbc" | "jdbc" => Some("odbc"),
        _ => match db_type.to_ascii_lowercase().as_str() {
            "mysql" => Some("mysql"),
            "postgres" | "postgresql" => Some("postgresql"),
            "sqlite" => Some("sqlite"),
            "sqlserver" | "mssql" => Some("sqlserver"),
            "oracle" => Some("oracle"),
            "odbc" | "jdbc" => Some("odbc"),
            _ => None,
        },
    }
}

fn dbx_tunnel(value: Option<&serde_json::Value>) -> Option<TunnelLayer> {
    let array = value?.as_array()?;
    for layer in array {
        let object = layer.as_object()?;
        if get_str(object, "type").as_deref() != Some("ssh") {
            continue;
        }
        let host = get_str(object, "host")?;
        let username = get_str(object, "user")?;
        if host.is_empty() || username.is_empty() {
            continue;
        }
        let auth_method = get_str(object, "auth_method").unwrap_or_default();
        let key_path = get_str(object, "key_path").unwrap_or_default();
        let uses_key = auth_method.eq_ignore_ascii_case("key")
            || (auth_method.is_empty() && !key_path.is_empty());
        return Some(TunnelLayer {
            kind: TunnelKind::Ssh,
            host,
            port: object
                .get("port")
                .and_then(json_u16)
                .filter(|p| *p > 0)
                .unwrap_or(22),
            username,
            password: if uses_key {
                String::new()
            } else {
                get_str(object, "password").unwrap_or_default()
            },
            key_path: if uses_key { key_path } else { String::new() },
            auth: if uses_key {
                TunnelAuth::KeyFile
            } else {
                TunnelAuth::Password
            },
        });
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

/// Normalize a key/type string: lower-case and drop every non-alphanumeric character, so
/// `SSH_Host`, `sshHost` and `ssh host` all collapse to `sshhost`.
fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// The first non-empty value among `keys`, trimmed.
fn pick(values: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = values.get(*key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Interpret a Navicat boolean flag (`1`/`true`/`yes`/`on`/`checked`).
fn truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(|value| value.trim().to_ascii_lowercase()),
        Some(ref value)
            if matches!(value.as_str(), "1" | "true" | "yes" | "y" | "on" | "checked")
    )
}

/// Read a string field from a JSON object (empty strings become `None`).
fn get_str(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Read a port that may be a JSON number or a numeric string.
fn json_u16(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
}

fn is_zero(value: &u16) -> bool {
    *value == 0
}

/// Accept both a JSON number and a numeric string for `port`.
fn port_from_any<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(json_u16(&value).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConnectionGroup;
    use cbc::cipher::BlockEncryptMut;

    /// Encrypt like Navicat: AES-128-CBC + PKCS7, hex-encoded (upper-case).
    fn encrypt_navicat(password: &str) -> String {
        type Enc = cbc::Encryptor<Aes128>;
        let ciphertext = Enc::new(NAVICAT_KEY.into(), NAVICAT_IV.into())
            .encrypt_padded_vec_mut::<Pkcs7>(password.as_bytes());
        hex::encode_upper(ciphertext)
    }

    #[test]
    fn rustgrid_bundle_round_trips_with_groups_and_protected_password() {
        let groups = vec![
            GroupEntry::Group(ConnectionGroup {
                id: 7,
                name: "Prod".to_string(),
                expanded: true,
                children: vec![
                    GroupEntry::Connection("conn-1".to_string()),
                    GroupEntry::Connection("conn-2".to_string()),
                ],
            }),
            GroupEntry::Connection("conn-3".to_string()),
        ];
        let file = build_rustgrid_file(
            vec![
                TransferConnection {
                    id: Some("conn-1".to_string()),
                    name: "Local MySQL".to_string(),
                    driver: "mysql".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 3306,
                    username: "root".to_string(),
                    database: Some("app".to_string()),
                    options: BTreeMap::from([("mysql.ssl".to_string(), "x".to_string())]),
                    settings: ConnectionOptions::default(),
                    password: Some("secret".to_string()),
                    color: Some("red".to_string()),
                },
                TransferConnection {
                    id: Some("conn-2".to_string()),
                    name: "B".to_string(),
                    driver: "sqlite".to_string(),
                    host: String::new(),
                    port: 0,
                    username: String::new(),
                    database: Some("/tmp/a.db".to_string()),
                    options: BTreeMap::new(),
                    settings: ConnectionOptions::default(),
                    password: None,
                    color: None,
                },
                TransferConnection {
                    id: Some("conn-3".to_string()),
                    name: "C".to_string(),
                    driver: "postgresql".to_string(),
                    host: "db".to_string(),
                    port: 5432,
                    username: "postgres".to_string(),
                    database: None,
                    options: BTreeMap::new(),
                    settings: ConnectionOptions::default(),
                    password: None,
                    color: None,
                },
            ],
            groups.clone(),
        );
        let text = serialize_rustgrid(&file).unwrap();
        // The password never appears in plaintext, but the rest of the file is readable JSON.
        assert!(!text.contains("secret"), "password must not be plaintext");
        assert!(text.contains("password_protection"));
        assert!(text.contains("\"builtin\""));

        let parsed = parse(&text, SourceFormat::RustGrid).unwrap();
        assert_eq!(parsed.connections, file.connections);
        assert_eq!(parsed.groups, groups);
        assert_eq!(parsed.skipped, 0);
        assert_eq!(parsed.passwords_failed, 0);
    }

    #[test]
    fn rustgrid_without_password_stays_password_free() {
        let file = build_rustgrid_file(
            vec![TransferConnection {
                id: None,
                name: "A".to_string(),
                driver: "postgresql".to_string(),
                host: "db".to_string(),
                port: 5432,
                username: "postgres".to_string(),
                database: None,
                options: BTreeMap::new(),
                settings: ConnectionOptions::default(),
                password: None,
                color: None,
            }],
            Vec::new(),
        );
        let text = serialize_rustgrid(&file).unwrap();
        // With no password there is nothing to protect, so the file is not flagged as protected.
        assert!(text.contains("\"password_protection\": \"none\""));
        let parsed = parse(&text, SourceFormat::RustGrid).unwrap();
        assert_eq!(parsed.connections[0].password, None);
        assert_eq!(parsed.passwords_failed, 0);
    }

    #[test]
    fn rustgrid_reports_a_password_that_cannot_be_decrypted() {
        let text = r#"{"format":"rustgrid-connections","version":1,"password_protection":"builtin","connections":[{"name":"A","driver":"mysql","password":"not-a-valid-token"}]}"#;
        let parsed = parse(text, SourceFormat::RustGrid).unwrap();
        assert_eq!(parsed.connections[0].password, None);
        assert_eq!(parsed.passwords_failed, 1);
    }

    #[test]
    fn rustgrid_rejects_foreign_format() {
        let text = r#"{"format":"other","version":1,"connections":[]}"#;
        assert!(parse(text, SourceFormat::RustGrid).is_err());
    }

    #[test]
    fn navicat_decrypts_password_and_maps_fields() {
        let password = encrypt_navicat("db-secret");
        let text = format!(
            r#"<Connections>
  <Connection ConnType="MYSQL" ConnectionName="prod" Host="10.0.0.1" Port="3307" UserName="root" Password="{password}" Database="app"/>
</Connections>"#
        );
        let parsed = parse(&text, SourceFormat::Navicat).unwrap();
        assert_eq!(parsed.connections.len(), 1);
        let connection = &parsed.connections[0];
        assert_eq!(connection.name, "prod");
        assert_eq!(connection.driver, "mysql");
        assert_eq!(connection.host, "10.0.0.1");
        assert_eq!(connection.port, 3307);
        assert_eq!(connection.username, "root");
        assert_eq!(connection.database, Some("app".to_string()));
        assert_eq!(connection.password, Some("db-secret".to_string()));
    }

    #[test]
    fn navicat_v1_and_v2_match_reference_samples() {
        // Both samples are from `how-does-navicat-encrypt-password` and decode to the same string.
        assert_eq!(
            decrypt_navicat_password("B75D320B6211468D63EB3B67C9E85933").as_deref(),
            Some("This is a test"),
            "Navicat 12+ (AES) sample"
        );
        assert_eq!(
            decrypt_navicat_password("0EA71F51DD37BFB60CCBA219BE3A").as_deref(),
            Some("This is a test"),
            "Navicat 11 (Blowfish) sample"
        );
    }

    #[test]
    fn navicat_v1_password_imports() {
        let text = r#"<Connections>
  <Connection ConnType="MYSQL" ConnectionName="legacy" Host="db" UserName="root" Password="0EA71F51DD37BFB60CCBA219BE3A"/>
</Connections>"#;
        let parsed = parse(text, SourceFormat::Navicat).unwrap();
        assert_eq!(
            parsed.connections[0].password.as_deref(),
            Some("This is a test")
        );
    }

    #[test]
    fn navicat_without_password_and_sqlite_path() {
        let text = r#"<Connections>
  <Connection ConnType="SQLite" Name="local" DatabaseFile="C:\data\demo.db"/>
  <Connection ConnType="POSTGRESQL" Name="pg" Host="db" Port="5432"/>
</Connections>"#;
        let parsed = parse(text, SourceFormat::Navicat).unwrap();
        assert_eq!(parsed.connections.len(), 2);
        assert_eq!(parsed.connections[0].driver, "sqlite");
        assert_eq!(
            parsed.connections[0].database,
            Some("C:\\data\\demo.db".to_string())
        );
        assert!(parsed.connections[0].host.is_empty());
        assert_eq!(parsed.connections[1].password, None);
    }

    #[test]
    fn navicat_skips_unsupported_engines() {
        let text = r#"<Connections>
  <Connection ConnType="MONGODB" Name="mongo" Host="localhost" Port="27017"/>
  <Connection ConnType="ORACLE" Name="ora" Host="db" Port="1521" Database="ORCL"/>
</Connections>"#;
        let parsed = parse(text, SourceFormat::Navicat).unwrap();
        assert_eq!(parsed.connections.len(), 1);
        assert_eq!(parsed.connections[0].driver, "oracle");
        assert_eq!(
            parsed.connections[0].options.get("oracle.connect_type"),
            Some(&"service".to_string())
        );
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn navicat_maps_ssh_tunnel() {
        let ssh_password = encrypt_navicat("ssh-secret");
        let text = format!(
            r#"<Connections>
  <Connection ConnType="MYSQL" Name="t" Host="db" SSH="true" SSH_Host="jump" SSH_Port="2202" SSH_UserName="deploy" SSH_AuthenMethod="PASSWORD" SSH_Password="{ssh_password}"/>
</Connections>"#
        );
        let parsed = parse(&text, SourceFormat::Navicat).unwrap();
        let tunnel = &parsed.connections[0].settings.tunnel;
        assert_eq!(tunnel.len(), 1);
        assert_eq!(tunnel[0].host, "jump");
        assert_eq!(tunnel[0].port, 2202);
        assert_eq!(tunnel[0].username, "deploy");
        assert_eq!(tunnel[0].password, "ssh-secret");
        assert_eq!(tunnel[0].auth, TunnelAuth::Password);
    }

    #[test]
    fn dbx_parses_array_and_config_bundle() {
        let text = r#"{"format":"dbx-config","connections":[
            {"name":"Local","db_type":"mysql","host":"127.0.0.1","port":3306,"username":"root","password":"pw","database":"app","color":"red"},
            {"name":"pg","db_type":"postgres","host":"db","port":"5432","username":"postgres"}
        ]}"#;
        let parsed = parse(text, SourceFormat::Dbx).unwrap();
        assert_eq!(parsed.connections.len(), 2);
        assert_eq!(parsed.connections[0].driver, "mysql");
        assert_eq!(parsed.connections[0].password, Some("pw".to_string()));
        assert_eq!(parsed.connections[0].color, Some("red".to_string()));
        assert_eq!(parsed.connections[1].driver, "postgresql");
        assert_eq!(parsed.connections[1].port, 5432);
    }

    #[test]
    fn dbx_sqlite_and_mariadb_profile() {
        let text = r#"[
            {"name":"db","db_type":"sqlite","host":"/tmp/app.db"},
            {"name":"maria","db_type":"mysql","driver_profile":"mariadb","host":"db","port":3306}
        ]"#;
        let parsed = parse(text, SourceFormat::Dbx).unwrap();
        assert_eq!(
            parsed.connections[0].database,
            Some("/tmp/app.db".to_string())
        );
        assert!(parsed.connections[0].host.is_empty());
        assert_eq!(parsed.connections[1].driver, "mariadb");
    }

    #[test]
    fn dbx_rejects_encrypted_payload() {
        let text = r#"{"format":"dbx-config-encrypted","data":"...."}"#;
        assert!(parse(text, SourceFormat::Dbx).is_err());
    }
}
