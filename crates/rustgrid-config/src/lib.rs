mod secrets;
pub mod transfer;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use rustgrid_core::{ConnectionProfile, SavedBackup, SavedQuery};
use serde::{Deserialize, Serialize};

const CURRENT_VERSION: u32 = 2;
const PROFILES_FILE: &str = "connections.json";
const SETTINGS_VERSION: u32 = 11;
const SETTINGS_FILE: &str = "settings.json";
const QUERIES_VERSION: u32 = 1;
const QUERIES_FILE: &str = "queries.json";
const QUERIES_DIR: &str = "queries";
const BACKUPS_VERSION: u32 = 1;
const BACKUPS_FILE: &str = "backups.json";
const BACKUPS_DIR: &str = "backups";
const DATABASES_VERSION: u32 = 1;
const DATABASES_FILE: &str = "databases.json";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine a configuration directory for this platform")]
    NoConfigDir,

    #[error("could not read or write configuration: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not parse configuration: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("could not encrypt or decrypt stored credentials")]
    Crypto,
}

pub type ConfigResult<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone)]
pub struct ConfigStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfilesFile {
    version: u32,
    #[serde(default)]
    profiles: Vec<ConnectionProfile>,
}

impl Default for ProfilesFile {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSetting {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LanguageSetting {
    #[default]
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
}

impl LanguageSetting {
    pub fn locale(self) -> &'static str {
        match self {
            LanguageSetting::En => "en",
            LanguageSetting::ZhCn => "zh-CN",
        }
    }
}

/// A table grid's remembered field layout (the 字段筛选 popup): the order its fields are shown in and
/// which of them are hidden. Persisted per connection/database/table, so a table opens with the
/// field set the user last chose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldLayout {
    /// Every field name in display order. A field the saved layout does not know (a column added to
    /// the table afterwards) is appended in the table's own order.
    #[serde(default)]
    pub order: Vec<String>,
    /// The fields currently hidden from the grid.
    #[serde(default)]
    pub hidden: Vec<String>,
}

/// One group in the connection tree. Groups nest: a group's `children` may hold both nested groups
/// and connections (referenced by their `ConnectionProfile::id`). Group names are not required to be
/// unique, so the numeric `id` is what identifies a group.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionGroup {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub children: Vec<GroupEntry>,
}

/// One ordered entry of the connection tree: either a nested group or a connection (by profile id).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum GroupEntry {
    Group(ConnectionGroup),
    /// A connection, identified by its `ConnectionProfile::id`.
    Connection(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    #[serde(default)]
    theme: ThemeSetting,
    #[serde(default)]
    language: LanguageSetting,
    /// Whether the right-hand object-info pane is shown. Remembered across launches; it is
    /// `false` until the user chooses to reveal it.
    #[serde(default)]
    show_info_pane: bool,
    /// The remembered list layout per page, keyed by a stable page id (e.g. `"queries"`).
    #[serde(default)]
    view_modes: BTreeMap<String, String>,
    /// The SQL editor font family; empty means the built-in default.
    #[serde(default)]
    editor_font_family: String,
    /// The SQL editor font size in px.
    #[serde(default = "default_editor_font_size")]
    editor_font_size: u32,
    #[serde(default = "default_true")]
    editor_line_numbers: bool,
    #[serde(default = "default_true")]
    editor_word_wrap: bool,
    /// Connection-profile ids whose tree node is expanded.
    #[serde(default)]
    expanded_connections: Vec<String>,
    /// Expanded database names per connection-profile id, for restoring the tree.
    #[serde(default)]
    expanded_databases: BTreeMap<String, Vec<String>>,
    /// The connection tree's layout: nested groups (and the connections they hold), in display
    /// order. Connections are stored by profile id so the layout survives reordering the profiles.
    #[serde(default)]
    groups: Vec<GroupEntry>,
    /// The per-connection environment colour (Navicat-style), keyed by profile id so it survives
    /// reordering/renaming. The value is a colour id (`"red"`, `"green"` or `"blue"`).
    #[serde(default)]
    connection_colors: BTreeMap<String, String>,
    /// The remembered field layout (字段筛选) per table grid, keyed by a
    /// `"<connection id>\u{1f}<database>\u{1f}<table>"` string.
    #[serde(default)]
    grid_field_layouts: BTreeMap<String, FieldLayout>,
    /// Whether the grid header shows each column's comment under its name.
    #[serde(default)]
    grid_show_comments: bool,
    /// Whether the grid header shows each column's data type between its name and comment.
    #[serde(default)]
    grid_show_types: bool,
    /// The keystroke that runs the whole query editor (Navicat's 运行), as a gpui keystroke string.
    #[serde(default = "default_run_shortcut")]
    run_shortcut: String,
    /// The keystroke that runs only the selected SQL (Navicat's 运行已选).
    #[serde(default = "default_run_selected_shortcut")]
    run_selected_shortcut: String,
}

/// The default SQL editor font size, in px (VS Code's default `editor.fontSize`).
pub const DEFAULT_EDITOR_FONT_SIZE: u32 = 14;

/// The default keystroke for 运行 (run the whole editor): Navicat's Ctrl+R. gpui's `secondary`
/// modifier is Ctrl off macOS and ⌘ on it, so the same default reads correctly on both.
pub const DEFAULT_RUN_SHORTCUT: &str = "secondary-r";

/// The default keystroke for 运行已选 (run the selection): Navicat's Ctrl+Shift+R.
pub const DEFAULT_RUN_SELECTED_SHORTCUT: &str = "secondary-shift-r";

fn default_editor_font_size() -> u32 {
    DEFAULT_EDITOR_FONT_SIZE
}

fn default_true() -> bool {
    true
}

fn default_run_shortcut() -> String {
    DEFAULT_RUN_SHORTCUT.to_string()
}

fn default_run_selected_shortcut() -> String {
    DEFAULT_RUN_SELECTED_SHORTCUT.to_string()
}

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub theme: ThemeSetting,
    pub language: LanguageSetting,
    pub show_info_pane: bool,
    pub view_modes: BTreeMap<String, String>,
    /// The SQL editor font family; empty means the built-in default.
    pub editor_font_family: String,
    /// The SQL editor font size in px.
    pub editor_font_size: u32,
    pub editor_line_numbers: bool,
    pub editor_word_wrap: bool,
    /// Connection-profile ids whose tree node is expanded.
    pub expanded_connections: Vec<String>,
    /// Expanded database names per connection-profile id.
    pub expanded_databases: BTreeMap<String, Vec<String>>,
    /// The connection tree's layout (nested groups and their connections).
    pub groups: Vec<GroupEntry>,
    /// The per-connection environment colour, keyed by connection-profile id. The value is a
    /// colour id (`"red"`, `"green"` or `"blue"`); a missing entry means no colour.
    pub connection_colors: BTreeMap<String, String>,
    /// The remembered field layout (字段筛选) per table grid, keyed by connection/database/table.
    pub grid_field_layouts: BTreeMap<String, FieldLayout>,
    /// Whether the grid header shows each column's comment under its name.
    pub grid_show_comments: bool,
    /// Whether the grid header shows each column's data type between its name and comment.
    pub grid_show_types: bool,
    /// The keystroke that runs the whole query editor (Navicat's 运行). A gpui keystroke string
    /// (`"secondary-r"`), so it is editable from the Options window's 快捷键 page.
    pub run_shortcut: String,
    /// The keystroke that runs only the selected SQL (Navicat's 运行已选).
    pub run_selected_shortcut: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeSetting::default(),
            language: LanguageSetting::default(),
            show_info_pane: false,
            view_modes: BTreeMap::new(),
            editor_font_family: String::new(),
            editor_font_size: DEFAULT_EDITOR_FONT_SIZE,
            editor_line_numbers: true,
            editor_word_wrap: true,
            expanded_connections: Vec::new(),
            expanded_databases: BTreeMap::new(),
            groups: Vec::new(),
            connection_colors: BTreeMap::new(),
            grid_field_layouts: BTreeMap::new(),
            grid_show_comments: false,
            grid_show_types: false,
            run_shortcut: default_run_shortcut(),
            run_selected_shortcut: default_run_selected_shortcut(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueriesFile {
    version: u32,
    #[serde(default)]
    queries: Vec<SavedQuery>,
}

impl Default for QueriesFile {
    fn default() -> Self {
        Self {
            version: QUERIES_VERSION,
            queries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupsFile {
    version: u32,
    #[serde(default)]
    backups: Vec<SavedBackup>,
}

/// The last database list seen for each connection (keyed by `ConnectionProfile::id`), so the save
/// dialog can offer databases without opening the connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatabasesFile {
    version: u32,
    #[serde(default)]
    databases: BTreeMap<String, Vec<String>>,
}

impl Default for BackupsFile {
    fn default() -> Self {
        Self {
            version: BACKUPS_VERSION,
            backups: Vec::new(),
        }
    }
}

impl ConfigStore {
    pub fn new() -> ConfigResult<Self> {
        let dirs = ProjectDirs::from("", "", "rustgrid").ok_or(ConfigError::NoConfigDir)?;
        Ok(Self {
            root: dirs.config_dir().to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn profiles_path(&self) -> PathBuf {
        self.root.join(PROFILES_FILE)
    }

    pub fn settings_path(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE)
    }

    pub fn queries_path(&self) -> PathBuf {
        self.root.join(QUERIES_FILE)
    }

    /// The root directory of saved queries. Each query is a `.sql` file under
    /// `<root>/queries/<connection_id>/<database>/<name>.sql`, or under an extra
    /// `<schema>/` segment when the query was saved while a schema was selected.
    pub fn queries_dir(&self) -> PathBuf {
        self.root.join(QUERIES_DIR)
    }

    /// The file a saved query is written to. A `schema` files the query one level deeper.
    pub fn query_file_path(
        &self,
        connection_id: &str,
        database: &str,
        schema: Option<&str>,
        name: &str,
    ) -> PathBuf {
        let mut path = self
            .queries_dir()
            .join(sanitize_component(connection_id))
            .join(sanitize_component(database));
        if let Some(schema) = schema.filter(|schema| !schema.is_empty()) {
            path = path.join(sanitize_component(schema));
        }
        path.join(format!("{}.sql", sanitize_component(name)))
    }

    /// Write (or overwrite) one saved query's `.sql` file.
    pub fn save_query_file(&self, query: &SavedQuery) -> ConfigResult<PathBuf> {
        let path = self.query_file_path(
            &query.connection_id,
            &query.database,
            query.schema.as_deref(),
            &query.name,
        );
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, &query.sql)?;
        Ok(path)
    }

    /// Remove one saved query's `.sql` file.
    pub fn delete_query_file(&self, query: &SavedQuery) -> ConfigResult<()> {
        let path = self.query_file_path(
            &query.connection_id,
            &query.database,
            query.schema.as_deref(),
            &query.name,
        );
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Move a set of legacy `queries.json` entries into the per-file tree (once). The old file is
    /// renamed to `queries.json.bak` so the migration does not run again.
    pub fn migrate_legacy_queries(&self) -> ConfigResult<()> {
        let path = self.queries_path();
        if !path.exists() {
            return Ok(());
        }
        if let Ok(contents) = fs::read_to_string(&path)
            && let Ok(file) = serde_json::from_str::<QueriesFile>(&contents)
        {
            for query in &file.queries {
                let _ = self.save_query_file(query);
            }
        }
        let _ = fs::rename(&path, self.root.join(format!("{QUERIES_FILE}.bak")));
        Ok(())
    }

    /// The root directory of this app's NB3 backups. Files live under
    /// `<root>/backups/<connection_id>/<database>/<name>.nb3`.
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join(BACKUPS_DIR)
    }

    pub fn load_profiles(&self) -> ConfigResult<Vec<ConnectionProfile>> {
        let path = self.profiles_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(path)?;
        let file: ProfilesFile = serde_json::from_str(&contents)?;
        Ok(migrate(file).profiles)
    }

    pub fn save_profiles(&self, profiles: &[ConnectionProfile]) -> ConfigResult<()> {
        fs::create_dir_all(&self.root)?;
        let file = ProfilesFile {
            version: CURRENT_VERSION,
            profiles: profiles.to_vec(),
        };
        let contents = serde_json::to_string_pretty(&file)?;
        fs::write(self.profiles_path(), contents)?;
        Ok(())
    }

    pub fn load_secrets(&self) -> ConfigResult<BTreeMap<String, String>> {
        secrets::load(&self.root)
    }

    pub fn save_secrets(&self, secrets: &BTreeMap<String, String>) -> ConfigResult<()> {
        secrets::save(&self.root, secrets)
    }

    pub fn load_settings(&self) -> ConfigResult<AppSettings> {
        let path = self.settings_path();
        if !path.exists() {
            return Ok(AppSettings::default());
        }
        let contents = fs::read_to_string(path)?;
        let file: SettingsFile = serde_json::from_str(&contents)?;
        Ok(AppSettings {
            theme: file.theme,
            language: file.language,
            show_info_pane: file.show_info_pane,
            view_modes: file.view_modes,
            editor_font_family: file.editor_font_family,
            editor_font_size: file.editor_font_size,
            editor_line_numbers: file.editor_line_numbers,
            editor_word_wrap: file.editor_word_wrap,
            expanded_connections: file.expanded_connections,
            expanded_databases: file.expanded_databases,
            groups: file.groups,
            connection_colors: file.connection_colors,
            grid_field_layouts: file.grid_field_layouts,
            grid_show_comments: file.grid_show_comments,
            grid_show_types: file.grid_show_types,
            run_shortcut: file.run_shortcut,
            run_selected_shortcut: file.run_selected_shortcut,
        })
    }

    pub fn save_settings(&self, settings: &AppSettings) -> ConfigResult<()> {
        fs::create_dir_all(&self.root)?;
        let file = SettingsFile {
            version: SETTINGS_VERSION,
            theme: settings.theme,
            language: settings.language,
            show_info_pane: settings.show_info_pane,
            view_modes: settings.view_modes.clone(),
            editor_font_family: settings.editor_font_family.clone(),
            editor_font_size: settings.editor_font_size,
            editor_line_numbers: settings.editor_line_numbers,
            editor_word_wrap: settings.editor_word_wrap,
            expanded_connections: settings.expanded_connections.clone(),
            expanded_databases: settings.expanded_databases.clone(),
            groups: settings.groups.clone(),
            connection_colors: settings.connection_colors.clone(),
            grid_field_layouts: settings.grid_field_layouts.clone(),
            grid_show_comments: settings.grid_show_comments,
            grid_show_types: settings.grid_show_types,
            run_shortcut: settings.run_shortcut.clone(),
            run_selected_shortcut: settings.run_selected_shortcut.clone(),
        };
        let contents = serde_json::to_string_pretty(&file)?;
        fs::write(self.settings_path(), contents)?;
        Ok(())
    }

    pub fn load_queries(&self) -> ConfigResult<Vec<SavedQuery>> {
        let path = self.queries_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(path)?;
        let file: QueriesFile = serde_json::from_str(&contents)?;
        Ok(file.queries)
    }

    pub fn save_queries(&self, queries: &[SavedQuery]) -> ConfigResult<()> {
        fs::create_dir_all(&self.root)?;
        let file = QueriesFile {
            version: QUERIES_VERSION,
            queries: queries.to_vec(),
        };
        let contents = serde_json::to_string_pretty(&file)?;
        fs::write(self.queries_path(), contents)?;
        Ok(())
    }

    pub fn load_backups(&self) -> ConfigResult<Vec<SavedBackup>> {
        let path = self.root.join(BACKUPS_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(path)?;
        let file: BackupsFile = serde_json::from_str(&contents)?;
        Ok(file.backups)
    }

    pub fn save_backups(&self, backups: &[SavedBackup]) -> ConfigResult<()> {
        fs::create_dir_all(&self.root)?;
        let file = BackupsFile {
            version: BACKUPS_VERSION,
            backups: backups.to_vec(),
        };
        let contents = serde_json::to_string_pretty(&file)?;
        fs::write(self.root.join(BACKUPS_FILE), contents)?;
        Ok(())
    }

    /// The remembered database names per connection profile id.
    pub fn load_database_cache(&self) -> ConfigResult<BTreeMap<String, Vec<String>>> {
        let path = self.root.join(DATABASES_FILE);
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let contents = fs::read_to_string(path)?;
        let file: DatabasesFile = serde_json::from_str(&contents)?;
        Ok(file.databases)
    }

    pub fn save_database_cache(
        &self,
        databases: &BTreeMap<String, Vec<String>>,
    ) -> ConfigResult<()> {
        fs::create_dir_all(&self.root)?;
        let file = DatabasesFile {
            version: DATABASES_VERSION,
            databases: databases.clone(),
        };
        let contents = serde_json::to_string_pretty(&file)?;
        fs::write(self.root.join(DATABASES_FILE), contents)?;
        Ok(())
    }
}

/// Make a filesystem-safe path component from a connection id or database name.
fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => character,
        })
        .collect()
}

fn migrate(mut file: ProfilesFile) -> ProfilesFile {
    if file.version < CURRENT_VERSION {
        file.version = CURRENT_VERSION;
    }
    file
}
