# Keyboard, query runner and session

RustGrid can be driven from the keyboard, runs scripts statement by statement like Navicat, and
reopens what you had open. Each section ends with a short **Try it** list for checking the feature
by hand.

## Shortcuts are commands

Every shortcut is a named command (`crates/rustgrid-app/src/app/keymap.rs`). **Options →
Shortcuts** lists them by area with a search box: click a command's field and press the new keys,
`↺` restores the default, `✕` removes it, and a conflict is flagged on both commands. Only the
shortcuts you changed are saved, in `settings.json` (`shortcuts`), and they apply at once — no
restart, no rebuild.

`Ctrl` is `⌘` on macOS unless noted.

| Area | Keys |
|---|---|
| Anywhere | `Ctrl+Q` new query (`⌘⇧N` on macOS) · `Ctrl+W` close tab · `Ctrl+Tab` / `Ctrl+Shift+Tab` (or `Ctrl+PageDown` / `Ctrl+PageUp`) next / previous tab, in the tab bar's order |
| Connection tree | `↑` `↓` · `PageUp` `PageDown` · `Home` `End` · `→` expand / enter · `←` collapse / go to parent · `Enter` open · `F2` rename · `Ctrl+A` · type a name to jump to it |
| Object list | `↑` `↓` (`←` `→` by column in the tile view) · `PageUp` `PageDown` · `Home` `End` · `Enter` open · `F2` rename · `Ctrl+A` · type a name to jump to it · `Esc` in the search box clears it |
| Table grid | `F5` refresh in place · arrows (`Shift` extends) · `PageUp` `PageDown` · `Home` `End` row start / end · `Ctrl+Home` `Ctrl+End` first / last cell · `Tab` `Shift+Tab` next / previous cell · `F2` value editor |
| Query editor | `Ctrl+R` run (the selection if any, else everything) · `Ctrl+Shift+R` or `Ctrl+Enter` run the selection, else the statement at the caret · `Ctrl+S` save · `Ctrl+K` or `Ctrl+/` toggle comment · `Ctrl+D` duplicate · `Ctrl+T` swap with the line above · `Alt+↑` `Alt+↓` move lines |

**Try it:** Options → Shortcuts → search "duplicate" → click its field → press `Ctrl+Shift+D` →
OK; in a query, `Ctrl+Shift+D` now duplicates the line. `↺` brings back `Ctrl+D`.

## Navigation

The connection tree and the object list move with the keys above and scroll to keep the selection
in view; both have a draggable scrollbar. Holding an arrow key repeats smoothly (only the list
redraws while the key is held). Switching tabs gives the keyboard focus back to that tab's pane —
the object list, the grid's cell or the editor — and the toolbar section follows the active tab;
the **Objects** tab lists the items of the current section (tables, views, queries, backups, …).

**Try it:** click a connection, hold `↓` through a long database list, type the first letters of a
table, press `Enter`. `Ctrl+Tab` back to the Objects tab and keep using the arrows.

## Table grid

The grid moves like a spreadsheet. A cell is edited by double-clicking it, clicking it again once
selected, pressing `Enter`, or typing; `←` `→` then move the caret inside the cell, while `↑` `↓`,
`Tab` and `Enter` save it and move on. `F5` reloads the page and keeps the selected cell.

`F2` opens **the value editor** (Navicat's): a panel under the grid with the whole value. It takes
the keyboard focus, follows the grid's cursor (a changed value is saved before it moves on),
`Ctrl+Enter` saves, `Esc` closes without saving, and `F2` again closes it.

**Try it:** open a table, `F2` on a long text cell, edit it, click another cell — the edit is saved
and the panel shows the new cell. `F2` closes it.

## Query editor

**Comments** follow Navicat: `--`, `//` and (on MySQL / MariaDB) `#` comment out the rest of the
line, with or without a following space, and `/*` comments out everything up to `*/` — or to the
end, while it is still unclosed. One lexer (`app/sql_lex.rs`) both colours the editor and decides
what is sent, so whatever looks commented out never runs: `//` and an unclosed `/*` are removed
before sending, and MySQL gets the space it requires after `--`. `#` is a comment only on MySQL /
MariaDB (PostgreSQL uses it as an operator, SQL Server for `#temp` tables).

`Ctrl+K` / `Ctrl+/` toggles a comment the way Navicat writes it — `# ` on MySQL / MariaDB, `-- `
elsewhere — and wraps part of a single line in `/* */`. **Format SQL** is undoable (`Ctrl+Z`), and
clicking it again before editing restores the original text.

**Try it:** type `SELECT 1; //SELECT 2;` and `/* SELECT 3;` on the next line: both look commented
out, and running the editor runs only `SELECT 1`.

## Running queries

A run executes its statements **one at a time on one pinned connection**, so `USE`, `SET` and
transactions carry from one to the next. Every statement reports its own result, error and time; a
failing statement does not hide the others (**Options → General → Stop running a script when a
statement fails** stops at the first one). MySQL scripts are split by RustGrid's lexer, which honours
the `mysql` client's `DELIMITER` command and keeps a routine's `BEGIN … END` body whole.

The **Info** tab is Navicat's message log in a read-only editor (select with the mouse or
`Shift`+arrows, copy with `Ctrl+C`): each statement, then right under it its rows / affected rows or
its error, in red, and its time. The part of the error the server points at (`near '…'`) is
emphasised and marked in the statement. It opens by itself when a statement failed. A timer next to
Run / Stop counts while the query runs; **Stop** is disabled until then.

**Options → General → Statement limit per run** (default 20, `0` = no limit) guards against running
a pasted dump by accident: a run executes the first statements up to the limit and skips the rest,
with a notice above the log whose **Change the limit** link opens the setting and points it out.

**Try it:** run `SELECT 1; SELEC 2; SELECT 3;` — results 1 and 3 have grids, the Info tab shows the
error under `SELEC 2` with `SELEC 2` marked. Set the limit to 2 and run it again: the notice says one
statement was skipped.

## Completion

Columns are loaded for the tables of every statement in the editor, the popup waits briefly for a
table's columns instead of opening on table names, and where a column is expected the statement's
columns and aliases rank above other tables.

**Try it:** type `SELECT ` then `FROM some_table WHERE `, then go back after `SELECT ` and type a
letter: that table's columns lead.

## Session

RustGrid saves what is open (`session.json`, about a second after a change, and on quit) and
reopens it on the next start:

- query tabs, with their connection, database and caret — including unsaved text;
- the connections that were open: reconnected at once if their password is saved, otherwise when
  you connect them (no password prompt at startup);
- the databases that were open in the tree, and the one the object list showed, so a new query
  defaults to it again;
- table and view tabs, reopened at their page once their connection is connected;
- the tab you were on.

Nothing is loaded from a database until its connection is back, so starting stays fast.

**Try it:** open a database, a table and a query with unsaved text, quit, start again.
