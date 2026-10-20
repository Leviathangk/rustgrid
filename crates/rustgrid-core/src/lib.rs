pub mod capability;
pub mod descriptor;
pub mod dialect;
pub mod driver;
pub mod error;
pub mod model;
pub mod options;
pub mod registry;
pub mod routine;
pub mod split;
pub mod sync;
pub mod user;
pub mod view;

pub use capability::{DriverCapabilities, DriverCapability};
pub use descriptor::{
    AdvancedFields, AutoIncrementRule, ConnectionFieldChoice, ConnectionFieldKind,
    ConnectionFieldLabel, ConnectionFieldSpec, ConnectionFormSpec, ConnectionHomePage,
    ConnectionPage, ConnectionStandardField, DesignerColumnOption, DesignerFieldColumn,
    DesignerIndexColumn, DesignerIndexKind, DesignerOptionField, DesignerTab, DriverDescriptor,
    DriverIconStyle, TableDesignerSpec,
};
pub use dialect::DriverDialect;
pub use driver::{
    Connection, DatabaseEditorSpec, DatabaseEditorTab, Driver, ScriptSession, UserEditorSpec,
};
pub use error::{Error, Result};
pub use model::{
    BackupObjectKind, CellValue, CheckDef, ColumnDef, ColumnInfo, ConnectionConfig,
    ConnectionProfile, DatabaseInfo, DatabaseOptions, DriverId, FilterCondition, FilterConjunction,
    FilterGroup, FilterNode, FilterOperator, ForeignKeyDef, IndexColumnDef, IndexDef, ObjectDump,
    ObjectKind, PageRequest, QueryResult, RowInsert, RowUpdate, SavedBackup, SavedBackupSelection,
    SavedQuery, SavedTransfer, SortColumn, TableInfo, TableOptions, TablePage, TableSchema,
    TableStatus, TransferMode, TriggerDef, UniqueKeyDef,
};
pub use options::{ConnectionOptions, TlsMode, TlsOptions, TunnelAuth, TunnelKind, TunnelLayer};
pub use registry::{BuiltinDriverSource, DriverRegistry, DriverSource};
pub use routine::{RoutineDetails, RoutineEdit, RoutineInfo, RoutineKind};
pub use sync::{
    CellChange, DataDiff, RowChange, RowChangeKind, SchemaChange, SchemaChangeKind,
    column_type_text, diff_columns, diff_rows, render_change_sql, render_literal,
};
pub use user::{
    AccountKind, DefaultObjectType, DefaultPrivilege, DefaultPrivilegeInfo, ObjectGrant,
    ObjectPrivilegeRow, PrivilegeCatalog, PrivilegeGroup, PrivilegeId, PrivilegeInfo,
    PrivilegePreset, PrivilegeScope, RoleMembership, SecurableClass, ServerSecurableGrant,
    UserAccount, UserDetails, UserEdit, UserEditSection, UserMapping,
};
pub use view::{ViewDetails, ViewEdit, ViewInfo};
