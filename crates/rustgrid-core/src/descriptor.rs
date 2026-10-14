//! Presentation and capability metadata for one driver.
//!
//! [`DriverDescriptor`] is the single source of truth for how the UI labels, icons, orders and
//! gates a driver. The app must not match on the engine id; it reads the descriptor instead.
//! [`Driver::descriptor`](crate::Driver::descriptor) builds one from the historical trait methods
//! by default, so adding it changes no behavior until an engine opts in.

use crate::capability::DriverCapabilities;
use crate::driver::DatabaseEditorSpec;
use crate::model::DriverId;

/// How a driver's icon is drawn in the connection tree and menus.
///
/// Every style paints the engine's mark as a white glyph on a solid 16px badge, so it stays legible
/// on a tinted (environment-coloured) row as well as on the plain background; the variants only
/// differ in the badge's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverIconStyle {
    /// The badge carries the connection status colour.
    Plain,
    /// The badge carries the connection status colour (the MySQL/MariaDB brand marks).
    SolidBadge,
    /// The badge carries the engine's brand colour while the connection is open, and the status
    /// colour while it is closed (SQLite, SQL Server, PostgreSQL, Oracle).
    Brand(u32),
}

/// A page (tab) of the New/Edit Connection window, and where an engine-specific field is shown.
///
/// The vocabulary is engine-agnostic: a driver declares which pages it shows and where its own
/// fields go, so the form never matches on an engine id (mirrors [`DatabaseEditorTab`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnectionPage {
    /// The first tab: the connection's identity (alias, network address or file, credentials).
    General,
    /// TLS / SSL.
    Tls,
    /// SSH / SOCKS5 / HTTP tunnel.
    Tunnel,
    /// Timeouts, read-only, session init SQL, plus engine fields placed here.
    Advanced,
}

impl ConnectionPage {
    /// i18n key of the page's tab label.
    pub fn label_key(self) -> &'static str {
        match self {
            ConnectionPage::General => "form.tab.general",
            ConnectionPage::Tls => "form.tab.tls",
            ConnectionPage::Tunnel => "form.tab.tunnel",
            ConnectionPage::Advanced => "form.tab.advanced",
        }
    }

    /// A stable id fragment for the tab's element id.
    pub fn id(self) -> &'static str {
        match self {
            ConnectionPage::General => "general",
            ConnectionPage::Tls => "tls",
            ConnectionPage::Tunnel => "tunnel",
            ConnectionPage::Advanced => "advanced",
        }
    }
}

/// The shape of the first (常规) page: which standard fields it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionHomePage {
    /// Alias + network address + port + credentials + default database (a network engine).
    Network,
    /// Alias + database file (a file-based engine, e.g. SQLite).
    File,
    /// Alias + the generic ODBC fields (driver name, DSN, connection string, engine hint).
    Odbc,
}

/// A standard form field whose label a driver may rename for its engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStandardField {
    Name,
    Host,
    Port,
    Username,
    Password,
    /// The "default database" field (Oracle renames it to a service name, SQL Server to the
    /// initial database, ...).
    Database,
    /// The file-based engine's database file.
    File,
    /// The 高级 page's connect timeout (SQLite renames it to its lock/busy timeout).
    ConnectTimeout,
    /// The 高级 page's query timeout.
    QueryTimeout,
    /// The 高级 page's keep-alive interval.
    Keepalive,
    /// The 高级 page's read-only toggle.
    ReadOnly,
    /// The 高级 page's session-init SQL.
    InitSql,
}

/// A per-engine rename/hint of one standard field. Fields a driver does not list keep the app's
/// default label.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionFieldLabel {
    pub field: ConnectionStandardField,
    /// i18n key of the field's label.
    pub label_key: &'static str,
    /// i18n key of the field's label-row hint, if any.
    pub hint_key: Option<&'static str>,
    /// Show the standard field only while `options[0]` is one of the listed values (`None` →
    /// always shown). Oracle uses it to hide 服务名/SID for the connection types that ignore it
    /// (`tns` / `connect_string`); an unset option counts as the controlling `Select`'s first
    /// choice.
    pub visible_when: Option<(&'static str, &'static [&'static str])>,
}

/// One choice of a [`ConnectionFieldKind::Select`] field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFieldChoice {
    /// The value stored in `ConnectionProfile::options`.
    pub value: &'static str,
    /// i18n key of the choice's label.
    pub label_key: &'static str,
}

/// How one engine-specific connection field is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionFieldKind {
    /// A plain single-line text field.
    Text,
    /// A masked single-line text field (a password or passphrase).
    Secret,
    /// A directory path with a trailing 浏览 (browse) button.
    Folder,
    /// A dropdown with a fixed set of choices; the first choice is the default.
    Select(Vec<ConnectionFieldChoice>),
}

/// One engine-specific field on the connection form.
///
/// The field reads and writes a single [`ConnectionProfile::options`] entry, so declaring a new
/// field needs no change to `connections.json` and no migration. Labels and hints are i18n keys
/// (a driver crate has no locale files of its own, so it hands the app the keys to resolve).
#[derive(Debug, Clone)]
pub struct ConnectionFieldSpec {
    /// The `ConnectionProfile::options` key this field reads and writes.
    pub key: &'static str,
    /// i18n key of the field's label.
    pub label_key: &'static str,
    /// i18n key of an optional hint shown on the label row.
    pub hint_key: Option<&'static str>,
    /// How the field is edited.
    pub kind: ConnectionFieldKind,
    /// Which page the field is shown on.
    pub page: ConnectionPage,
    /// Show the field only while `profile.options[0]` equals `1` (e.g. Oracle's TNS alias field is
    /// shown while the connection type is `tns`). A field that already holds a value is always
    /// shown, so a legacy profile can never hide an active option.
    pub visible_when: Option<(&'static str, &'static str)>,
}

/// Which of the standard 高级 fields an engine actually honors. The form hides the ones an engine
/// leaves off, so a driver never shows a dead knob (e.g. Oracle has no keep-alive or connect
/// timeout, ODBC reads none of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvancedFields {
    /// 连接超时 / connect timeout.
    pub connect_timeout: bool,
    /// 查询超时 / per-statement query timeout.
    pub query_timeout: bool,
    /// 保持连接 / TCP keep-alive.
    pub keepalive: bool,
    /// 只读安全模式 / read-only session.
    pub read_only: bool,
    /// 连接初始化 SQL / session init SQL.
    pub init_sql: bool,
}

impl AdvancedFields {
    /// Every standard 高级 field (MySQL, MariaDB, PostgreSQL, SQL Server).
    pub const ALL: Self = Self {
        connect_timeout: true,
        query_timeout: true,
        keepalive: true,
        read_only: true,
        init_sql: true,
    };

    /// None of them (the generic ODBC driver reads no advanced settings).
    pub const NONE: Self = Self {
        connect_timeout: false,
        query_timeout: false,
        keepalive: false,
        read_only: false,
        init_sql: false,
    };
}

impl Default for AdvancedFields {
    fn default() -> Self {
        Self::ALL
    }
}

/// Which pages the connection form shows for an engine, how its 常规 page is shaped, how it renames
/// the standard fields, and where its own option fields go. The form stays engine-agnostic by
/// reading this instead of matching on the engine id.
#[derive(Debug, Clone)]
pub struct ConnectionFormSpec {
    /// The shape of the 常规 page.
    pub home: ConnectionHomePage,
    /// The tabs shown after 常规, in order.
    pub tabs: Vec<ConnectionPage>,
    /// Per-engine renames/hints for the standard fields.
    pub labels: Vec<ConnectionFieldLabel>,
    /// Engine-specific option fields, each placed on a page.
    pub options: Vec<ConnectionFieldSpec>,
    /// Which standard 高级 fields the engine honors; the rest are hidden.
    pub advanced: AdvancedFields,
}

impl Default for ConnectionFormSpec {
    /// The standard network form: 常规 + TLS + 隧道 + 高级, with no engine fields. This is what a
    /// plain network engine (MySQL, PostgreSQL, ...) shows.
    fn default() -> Self {
        Self {
            home: ConnectionHomePage::Network,
            tabs: vec![
                ConnectionPage::Tls,
                ConnectionPage::Tunnel,
                ConnectionPage::Advanced,
            ],
            labels: Vec::new(),
            options: Vec::new(),
            advanced: AdvancedFields::ALL,
        }
    }
}

/// Presentation and capability metadata for one driver.
#[derive(Debug, Clone)]
pub struct DriverDescriptor {
    /// The engine id (e.g. `sqlserver`).
    pub id: DriverId,
    /// The user-facing engine name (e.g. `SQL Server`).
    pub display_name: String,
    /// The port pre-filled by the connection form.
    pub default_port: u16,
    /// Whether the engine connects to a file rather than a network server.
    pub is_file_based: bool,
    /// The embedded asset path of the engine's icon (e.g. `icons/sqlserver.svg`).
    pub icon: &'static str,
    /// How the icon is drawn in the connection tree.
    pub icon_style: DriverIconStyle,
    /// The capabilities the engine advertises to the UI.
    pub capabilities: DriverCapabilities,
    /// Which fields and tabs the New/Edit Database dialog shows for this engine.
    pub database_editor: DatabaseEditorSpec,
    /// Which extra fields the New/Edit Connection form shows for this engine.
    pub connection_form: ConnectionFormSpec,
    /// Sort key for the New Connection menu (lower first); ties fall back to the display name.
    pub order: u16,
}
