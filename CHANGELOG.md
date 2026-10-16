# Changelog

English | [简体中文](CHANGELOG_zh.md)

All notable changes to RustGrid are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

UI labels are quoted as the app shows them, so this file quotes the English strings;
[`CHANGELOG_zh.md`](CHANGELOG_zh.md) quotes the 简体中文 ones.

## [0.0.13] - 2026-10-16

### Added

- **An engine-shaped table designer.** A driver now declares what its engine's designer shows —
  which sub-tabs, which per-column controls, which field-list columns and which table options
  (`DriverDescriptor::table_designer`) — and the designer renders that spec instead of one
  hard-coded MySQL layout. SQL Server therefore gets its own designer: Collation and Sparse in the
  field details panel instead of Auto Increment / Unsigned / Fill Zero, the Size / Scale / Computed
  field columns, and the new **Unique Keys** and **Checks** tabs. SQLite drops Options and Comment
  (it has neither), PostgreSQL's Options keeps the comment only, and Oracle drops Options.
- SQL Server's designer reads, edits and writes unique constraints (`sys.key_constraints`) and
  check constraints (`sys.check_constraints`), building them into `CREATE TABLE` and reconciling
  them with `ALTER TABLE … ADD/DROP CONSTRAINT`, plus a per-column collation (`COLLATE`), `SPARSE`
  columns and computed columns (`AS (…) [PERSISTED]`).
- **The index field editor is a draft panel**, like Navicat's: the chosen fields in order, each with
  the engine's own per-column options — Sort (`ASC` / `DESC`) on every engine and MySQL/MariaDB's
  Prefix (index only a leading part of the column, `code(10)`) — with ↑ ↓ + − on the left and Cancel
  / OK on the right. Every edit (name, sort order, prefix, order, add, remove) stays on the draft
  until OK, so the grid never shows a half-made change. The Primary Key row opens it as well: its
  columns are the Primary Key flags from the Fields tab, so that draft edits those flags and carries
  no per-column options.

### Changed

- An index column carries its own options now: `IndexDef`'s columns are `IndexColumnDef`
  (`descending`, `prefix_length`) instead of plain names, so `DESC` and MySQL's prefix survive a
  round trip. They are read from `information_schema.statistics` (`COLLATION`, `SUB_PART`),
  `sys.index_columns.is_descending_key`, `pg_index.indoption`, `all_ind_columns.descend` and
  `PRAGMA index_xinfo`, and written back as part of the column (`code(10) DESC`).
- The designer's field cells (the Fields cell under Indexes, the Fields and Referenced Fields cells
  under Foreign Keys, and the Fields cell under Unique Keys) are drop-downs: clicking anywhere in
  the cell opens the field list — the "..." button is gone. A foreign key's or unique key's list
  keeps its check boxes, since those hold more than one column, and every cell opens flush under the
  row you clicked (each row records its own anchor).
- The designer's field details panel, field list and Options page are no longer written against
  MySQL: `ColumnDef` carries `sparse` and `TableSchema` carries `unique_keys` / `checks`, and each
  engine fills in only what it has.

### Fixed

- **The table designer's grids scroll with the mouse wheel.** Its field/index/foreign-key (and the
  new unique-key/check) lists never moved under the wheel: a gpui `uniform_list` registers no
  scroll-wheel handler of its own, so its offset has to be driven through `scroll_to_item` — which
  is exactly what the table grid's wheel does. The designer now uses that same interaction: a notch
  moves the selected row by one and brings it into view, a horizontal gesture pans the columns, and
  a gesture aimed at an overlay above the grid (a drop-down, a picker popup) is left to it. The
  vertical scrollbar's thumb follows the rows and dragging it lands on the row at that position.
- **Auto Increment is offered only where the engine accepts it.** Every engine restricts it — SQL
  Server has a single `IDENTITY` column per table, only on a numeric type, and `ALTER COLUMN` cannot
  give a column the table already has one — so the field details panel **leaves the control out**
  rather than showing a dead one (it is there on a newly added column, or in a new table). Ticking it
  takes it off whichever column had it, and changing a column to a type that cannot carry one drops
  it, so the SQL Preview never shows an invalid statement.

## [0.0.12] - 2026-10-15

### Added

- **Changelog** in Options → About: once a check has found a newer release, the panel shows that
  version's notes — what installing would bring, in the UI's own language — and stays off the page
  entirely while this build already is the newest release. The repository keeps its changelog in
  English and Chinese, and the release job publishes each version's section as the release notes
  (it fails when a version's section is missing).
- SQL completion matches loosely: a candidate is found by its exact name, a prefix, anything inside
  it or a subsequence of it, with the closest matches leading; the tables lead right after `FROM` /
  `JOIN` / `INTO` / `UPDATE` / `TABLE`, and the everyday keywords (in their most-typed order) come
  before the rest of the language. `Tab` accepts the highlighted candidate, and still indents when
  no popup is open.
- The query editor's run keystrokes are configurable in Options → **Shortcuts**: click a field, press
  the combination, and OK applies it (Default restores Navicat's Ctrl+R / Ctrl+Shift+R). Ctrl+R runs
  the whole editor, Ctrl+Shift+R only the selection, and Ctrl+Enter stays an alias for running.

### Fixed

- Grid cells and headers have 6px of horizontal padding, so a value no longer sits flush against the
  previous column's divider and its text (issue #5).
- The MySQL account editor lists a database the account only holds an entry for
  (`GRANT USAGE ON db.* WITH GRANT OPTION`), which Navicat and `SHOW DATABASES` report as well.
- **Close Connection** closes that connection's account window and Object Privileges window with it
  and drops the Users list, so no window is left editing an account over a dead connection.

## [0.0.11] - 2026-10-15

### Added

- Grid **Field Filter** popup: hide, reorder (drag handle) and search fields, with Reset /
  Select All / Invert; edits land on a draft and only OK applies them, and the layout is remembered
  per connection / database / table.
- Field comments and data types in the grid header, controlled by the new **Data** page of the
  Options window (Show field comments / Show field types, both off by default).

### Fixed

- Field Filter now applies the draft only on OK (Cancel / click-away discards), uses one state-aware
  icon, opens a taller popup, and Invert never hides the last visible field.
- The tree's **Refresh** re-fetches the open database's tables and routines, so external changes
  show up.
- Run SQL File releases its pinned session inside the Tokio runtime instead of crashing when a
  script finishes.
- Read-only errors and SQL text are selectable/copyable; a submenu flyout closes when the pointer
  leaves it; filter and tree interaction polish; an empty grid renders as a full empty table.

## [0.0.10] - 2026-10-14

### Added

- Connection tree multi-select, drag-and-drop into groups, and **Import Connections**
  (RustGrid / Navicat / DBX) plus **Export Connections** in the connection menus.
- macOS ships a `.app` bundle with a Dock icon, and the updater swaps the bundle as one.

### Fixed

- The grid keeps its selection while the scrollbar thumb is dragged.
- Search placeholders retranslate when the language changes.
- Docs: added the README screenshot grid.

## [0.0.9] - 2026-10-13

### Fixed

- Grid cursor-wheel scrolling, single-line cells, backup pause, and multi-row delete.

## [0.0.8] - 2026-10-13

### Added

- Per-connection environment colour (Red / Yellow / Green) that paints the connection node and its whole
  subtree in the tree, plus every tab of that connection.

### Fixed

- The account editor requires a new password when the authentication plugin changes.

## [0.0.7] - 2026-10-12

### Changed

- Upgraded gpui-kit to 0.7.3 and adopted its new components.

### Fixed

- The custom titlebar stays clear of the macOS window buttons.

## [0.0.6] - 2026-10-12

### Added

- Per-engine connection pages and engine-specific options (General / TLS / Tunnel/Proxy / Advanced), driven by the
  driver descriptor.
- Dump SQL file and Run SQL File, with one pinned session per script and a live log box.
- Connection groups (nested, rename, delete, drag-reorder) and multi-row table delete.
- Database dump/run SQL file, schema double-click, and dialog / log fixes.

### Fixed

- Connection type stays directly above the field it controls; the dump repeats the INSERT header on
  every batch; Copy Log placement; record counts in the run log.

## [0.0.5] - 2026-10-10

### Fixed

- Keep the tree selection when switching main tabs, gate schema objects, and widen the filter dropdown.
- PostgreSQL: decode `"char"` catalog columns as text.

## [0.0.4] - 2026-10-10

### Added

- Live download progress for the self-updater.

### Fixed

- Schema gating, cleaner chrome, and arrow-key cell navigation.
- Grid paste into new rows, backup dialog polish, and the search input.

## [0.0.3] - 2026-10-09

### Added

- MySQL **Advanced** section for SSL / REQUIRE in the account editor.

### Changed

- UX overhaul and a new app icon; table-designer edit fix.

### Fixed

- Decode the unsigned `password_lifetime` column; backup profile rename; only show the applicable
  schema action.

## [0.0.2] - 2026-10-09

### Added

- Engine-aware account and privilege management: SQL Server user mapping, securables and full login
  attributes, and `WITH GRANT OPTION` for server privileges.
- Open / close a schema in the tree and highlight the open schema.

### Fixed

- Guarded `CREATE USER` and server-scope grants run in `master`; server-securables matrix alignment
  and grant/deny cycling; column overlap in the server privileges grid.

## [0.0.1] - 2026-10-08

First release.

### Added

- Cargo workspace scaffold: engine-agnostic `rustgrid-core`, a MySQL driver, the config store and
  the GPUI application; licensed under Apache-2.0.
- Connection management (create / edit / delete, connect, password prompt, encrypted secrets) and
  database management (charset / collation).
- Tables and views with a paginated, editable grid — inserts, multi-range Excel-like selection,
  sorting, a filter builder, resizable columns and a sum of the selected numeric cells.
- SQL editor with syntax highlighting, keyword/table completion, Beautify SQL, Explain, saved named
  queries, multi-result tabs, and an Extract SQL window.
- Backup / restore with RustGrid's own `.rgbak` container, and the import / export wizards
  (Excel / CSV / TXT / `.xlsx` / `.sql`).
- User and privilege management, stored-routine and view designers, and a unified Explorer-style
  list interaction shared by every list page.
- Drivers: MySQL / MariaDB, SQLite, SQL Server, PostgreSQL, Oracle, and a runtime-loaded generic
  ODBC engine (DB2, Dameng, ...).
- gpui-kit / shadcn chrome with light, dark and system themes, in English and 简体中文.
- Portable GitHub Releases builds with a self-updater.

[0.0.11]: https://github.com/yisier/rustgrid/releases/tag/v0.0.11
[0.0.10]: https://github.com/yisier/rustgrid/releases/tag/v0.0.10
[0.0.9]: https://github.com/yisier/rustgrid/releases/tag/v0.0.9
[0.0.8]: https://github.com/yisier/rustgrid/releases/tag/v0.0.8
[0.0.7]: https://github.com/yisier/rustgrid/releases/tag/v0.0.7
[0.0.6]: https://github.com/yisier/rustgrid/releases/tag/v0.0.6
[0.0.5]: https://github.com/yisier/rustgrid/releases/tag/v0.0.5
[0.0.4]: https://github.com/yisier/rustgrid/releases/tag/v0.0.4
[0.0.3]: https://github.com/yisier/rustgrid/releases/tag/v0.0.3
[0.0.2]: https://github.com/yisier/rustgrid/releases/tag/v0.0.2
[0.0.1]: https://github.com/yisier/rustgrid/releases/tag/v0.0.1
