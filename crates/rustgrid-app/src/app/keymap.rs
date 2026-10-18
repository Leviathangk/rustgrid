//! The app's keymap: every rebindable command, its key context and its built-in keystrokes.
//!
//! The Options window's 快捷键 page lists [`COMMANDS`], and the user's overrides (command id ->
//! keystrokes, stored in `settings.json`) replace a command's defaults. [`bind_keymap`] installs the
//! result. gpui's keymap has no removal API, so a re-bind first shadows every keystroke the previous
//! call bound with a `NoAction` in the same context; a later binding outranks an earlier one for
//! the same keystroke, so only the app's stale bindings stop firing and the kit's own keep working.
//!
//! Mechanics that must not move (Tab / Up / Down inside a cell editor, Tab accepting a completion)
//! are bound in `AppView::new` and are not commands.

use std::collections::BTreeMap;

use gpui::{App, KeyBinding, Keystroke, NoAction};
use gpui_kit::component::input::ShowCompletions;

use super::{
    BACKUP_LIST_CONTEXT, CopyBackupFile, CopyQueryFile, CopyStatusSql, DESIGNER_CONTEXT,
    GRID_STATUS_CONTEXT, PasteBackupFile, PasteQueryFile, QUERY_EDITOR_CONTEXT, QUERY_LIST_CONTEXT,
    RenameBackupFile, RenameQueryFile, RunQuery, RunSelectedQuery, SaveDesign, SaveQuery,
    SelectAllBackups, SelectAllOccurrences, SelectAllQueries, SelectAllStatusSql,
};

/// Where a command lives, for grouping the 快捷键 page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandGroup {
    QueryEditor,
    Designer,
    Lists,
}

impl CommandGroup {
    pub(super) const ALL: [CommandGroup; 3] = [
        CommandGroup::QueryEditor,
        CommandGroup::Designer,
        CommandGroup::Lists,
    ];

    /// The i18n key of the group's heading.
    pub(super) fn label_key(self) -> &'static str {
        match self {
            CommandGroup::QueryEditor => "keymap.group.query_editor",
            CommandGroup::Designer => "keymap.group.designer",
            CommandGroup::Lists => "keymap.group.lists",
        }
    }
}

/// One rebindable command.
pub(super) struct Command {
    /// The stable id stored in `settings.json` (`"query.run"`).
    pub id: &'static str,
    /// The i18n key of its label on the 快捷键 page.
    pub label_key: &'static str,
    pub group: CommandGroup,
    /// The gpui key context the binding is registered in.
    pub context: &'static str,
    /// Built-in keystrokes (gpui syntax; `secondary` is Ctrl off macOS and ⌘ on it).
    pub defaults: &'static [&'static str],
    /// Built-in keystrokes on macOS, where they differ from `defaults`.
    pub mac_defaults: Option<&'static [&'static str]>,
    /// Builds the binding of `keystroke` in `context`.
    pub bind: fn(&str, &'static str) -> KeyBinding,
}

impl Command {
    /// This platform's built-in keystrokes.
    pub(super) fn default_keys(&self) -> &'static [&'static str] {
        match self.mac_defaults {
            Some(keys) if cfg!(target_os = "macos") => keys,
            _ => self.defaults,
        }
    }
}

macro_rules! bind {
    ($action:expr) => {
        |keystroke, context| KeyBinding::new(keystroke, $action, Some(context))
    };
}

/// Every rebindable command, in 快捷键-page order within each group.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        id: "query.run",
        label_key: "query.run",
        group: CommandGroup::QueryEditor,
        context: QUERY_EDITOR_CONTEXT,
        defaults: &["secondary-r"],
        mac_defaults: None,
        bind: bind!(RunQuery),
    },
    Command {
        id: "query.run_selected",
        label_key: "query.run_selected",
        group: CommandGroup::QueryEditor,
        context: QUERY_EDITOR_CONTEXT,
        defaults: &["secondary-shift-r", "secondary-enter"],
        mac_defaults: None,
        bind: bind!(RunSelectedQuery),
    },
    Command {
        id: "query.save",
        label_key: "keymap.query.save",
        group: CommandGroup::QueryEditor,
        context: QUERY_EDITOR_CONTEXT,
        defaults: &["secondary-s"],
        mac_defaults: None,
        bind: bind!(SaveQuery),
    },
    Command {
        id: "query.show_completions",
        label_key: "keymap.query.show_completions",
        group: CommandGroup::QueryEditor,
        context: QUERY_EDITOR_CONTEXT,
        defaults: &["ctrl-space"],
        mac_defaults: None,
        bind: bind!(ShowCompletions),
    },
    Command {
        id: "query.select_all_occurrences",
        label_key: "keymap.query.select_all_occurrences",
        group: CommandGroup::QueryEditor,
        context: QUERY_EDITOR_CONTEXT,
        defaults: &["secondary-shift-l"],
        mac_defaults: None,
        bind: bind!(SelectAllOccurrences),
    },
    Command {
        id: "designer.save",
        label_key: "keymap.designer.save",
        group: CommandGroup::Designer,
        context: DESIGNER_CONTEXT,
        defaults: &["secondary-s"],
        mac_defaults: None,
        bind: bind!(SaveDesign),
    },
    Command {
        id: "backups.rename",
        label_key: "keymap.backups.rename",
        group: CommandGroup::Lists,
        context: BACKUP_LIST_CONTEXT,
        defaults: &["f2"],
        mac_defaults: None,
        bind: bind!(RenameBackupFile),
    },
    Command {
        id: "backups.copy",
        label_key: "keymap.backups.copy",
        group: CommandGroup::Lists,
        context: BACKUP_LIST_CONTEXT,
        defaults: &["secondary-c"],
        mac_defaults: None,
        bind: bind!(CopyBackupFile),
    },
    Command {
        id: "backups.paste",
        label_key: "keymap.backups.paste",
        group: CommandGroup::Lists,
        context: BACKUP_LIST_CONTEXT,
        defaults: &["secondary-v"],
        mac_defaults: None,
        bind: bind!(PasteBackupFile),
    },
    Command {
        id: "backups.select_all",
        label_key: "keymap.backups.select_all",
        group: CommandGroup::Lists,
        context: BACKUP_LIST_CONTEXT,
        defaults: &["secondary-a"],
        mac_defaults: None,
        bind: bind!(SelectAllBackups),
    },
    Command {
        id: "queries.rename",
        label_key: "keymap.queries.rename",
        group: CommandGroup::Lists,
        context: QUERY_LIST_CONTEXT,
        defaults: &["f2"],
        mac_defaults: None,
        bind: bind!(RenameQueryFile),
    },
    Command {
        id: "queries.copy",
        label_key: "keymap.queries.copy",
        group: CommandGroup::Lists,
        context: QUERY_LIST_CONTEXT,
        defaults: &["secondary-c"],
        mac_defaults: None,
        bind: bind!(CopyQueryFile),
    },
    Command {
        id: "queries.paste",
        label_key: "keymap.queries.paste",
        group: CommandGroup::Lists,
        context: QUERY_LIST_CONTEXT,
        defaults: &["secondary-v"],
        mac_defaults: None,
        bind: bind!(PasteQueryFile),
    },
    Command {
        id: "queries.select_all",
        label_key: "keymap.queries.select_all",
        group: CommandGroup::Lists,
        context: QUERY_LIST_CONTEXT,
        defaults: &["secondary-a"],
        mac_defaults: None,
        bind: bind!(SelectAllQueries),
    },
    Command {
        id: "status_sql.copy",
        label_key: "keymap.status_sql.copy",
        group: CommandGroup::Lists,
        context: GRID_STATUS_CONTEXT,
        defaults: &["secondary-c"],
        mac_defaults: None,
        bind: bind!(CopyStatusSql),
    },
    Command {
        id: "status_sql.select_all",
        label_key: "keymap.status_sql.select_all",
        group: CommandGroup::Lists,
        context: GRID_STATUS_CONTEXT,
        defaults: &["secondary-a"],
        mac_defaults: None,
        bind: bind!(SelectAllStatusSql),
    },
];

/// The command with `id`, if it exists.
pub(super) fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.id == id)
}

/// The keystrokes `command` is bound to: the user's override, or its built-in ones.
pub(super) fn keys_for(
    command: &Command,
    overrides: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    match overrides.get(command.id) {
        Some(keys) => keys.clone(),
        None => command
            .default_keys()
            .iter()
            .map(|key| key.to_string())
            .collect(),
    }
}

/// Store `keys` as `command`'s binding in `overrides`: equal to the built-in keys means "no
/// override", so a reset command stops being listed in `settings.json`.
pub(super) fn set_keys(
    command: &Command,
    keys: Vec<String>,
    overrides: &mut BTreeMap<String, Vec<String>>,
) {
    let defaults: Vec<String> = command
        .default_keys()
        .iter()
        .map(|key| key.to_string())
        .collect();
    if keys == defaults {
        overrides.remove(command.id);
    } else {
        overrides.insert(command.id.to_string(), keys);
    }
}

/// A keystroke in a canonical form for comparison (`secondary-r` and `ctrl-r` are the same key on
/// Windows), or `None` when it does not parse.
fn canonical(keystroke: &str) -> Option<String> {
    Keystroke::parse(keystroke).ok().map(|key| {
        let modifiers = key.modifiers;
        let mut parts = Vec::new();
        if modifiers.control {
            parts.push("ctrl");
        }
        if modifiers.alt {
            parts.push("alt");
        }
        if modifiers.shift {
            parts.push("shift");
        }
        if modifiers.platform {
            parts.push("cmd");
        }
        if modifiers.function {
            parts.push("fn");
        }
        parts.push(&key.key);
        parts.join("-")
    })
}

/// The other commands that `command` shares a keystroke with in the same key context, given
/// `overrides`. Only one of them would fire, so the 快捷键 page flags them.
pub(super) fn conflicts(
    command: &Command,
    overrides: &BTreeMap<String, Vec<String>>,
) -> Vec<&'static Command> {
    let mine: Vec<String> = keys_for(command, overrides)
        .iter()
        .filter_map(|key| canonical(key))
        .collect();
    if mine.is_empty() {
        return Vec::new();
    }
    COMMANDS
        .iter()
        .filter(|other| other.id != command.id && other.context == command.context)
        .filter(|other| {
            keys_for(other, overrides)
                .iter()
                .filter_map(|key| canonical(key))
                .any(|key| mine.contains(&key))
        })
        .collect()
}

/// The keystrokes [`bind_keymap`] registered, each with its key context, so the next call can
/// shadow them.
pub(super) type BoundKeys = Vec<(String, &'static str)>;

/// Bind every command to its keystrokes given `overrides`, first shadowing `previous` (what the
/// last call bound) with `NoAction`. A keystroke that does not parse is skipped rather than
/// panicking. Returns what is now bound.
pub(super) fn bind_keymap(
    cx: &mut App,
    overrides: &BTreeMap<String, Vec<String>>,
    previous: &[(String, &'static str)],
) -> BoundKeys {
    let mut bindings = Vec::new();
    for (keystroke, context) in previous {
        if Keystroke::parse(keystroke).is_ok() {
            bindings.push(KeyBinding::new(keystroke, NoAction {}, Some(context)));
        }
    }
    let mut bound = Vec::new();
    for command in COMMANDS {
        for keystroke in keys_for(command, overrides) {
            if Keystroke::parse(&keystroke).is_ok() {
                bindings.push((command.bind)(&keystroke, command.context));
                bound.push((keystroke, command.context));
            }
        }
    }
    cx.bind_keys(bindings);
    bound
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn command_ids_are_unique_and_defaults_parse() {
        for (index, command) in COMMANDS.iter().enumerate() {
            assert!(
                COMMANDS[index + 1..]
                    .iter()
                    .all(|other| other.id != command.id),
                "duplicate command id {}",
                command.id
            );
            for key in command
                .defaults
                .iter()
                .chain(command.mac_defaults.unwrap_or(&[]))
            {
                assert!(
                    Keystroke::parse(key).is_ok(),
                    "{}: bad default {key}",
                    command.id
                );
            }
        }
    }

    #[test]
    fn built_in_keystrokes_do_not_conflict() {
        let none = BTreeMap::new();
        for command in COMMANDS {
            let clashes: Vec<&str> = conflicts(command, &none).iter().map(|c| c.id).collect();
            assert!(
                clashes.is_empty(),
                "{} clashes with {clashes:?}",
                command.id
            );
        }
    }

    #[test]
    fn an_override_replaces_the_defaults_and_resetting_removes_it() {
        let run = command("query.run").expect("query.run");
        let mut overrides = BTreeMap::new();
        set_keys(run, keys(&["f9"]), &mut overrides);
        assert_eq!(keys_for(run, &overrides), keys(&["f9"]));

        set_keys(run, keys(&["secondary-r"]), &mut overrides);
        assert!(overrides.is_empty());
        assert_eq!(keys_for(run, &overrides), keys(&["secondary-r"]));
    }

    #[test]
    fn an_empty_override_unbinds_the_command() {
        let save = command("query.save").expect("query.save");
        let mut overrides = BTreeMap::new();
        set_keys(save, Vec::new(), &mut overrides);
        assert!(keys_for(save, &overrides).is_empty());
        assert!(conflicts(save, &overrides).is_empty());
    }

    #[test]
    fn the_same_key_in_one_context_is_a_conflict_but_not_across_contexts() {
        let run = command("query.run").expect("query.run");
        let save = command("query.save").expect("query.save");
        let designer_save = command("designer.save").expect("designer.save");
        let mut overrides = BTreeMap::new();
        set_keys(run, keys(&["secondary-s"]), &mut overrides);

        let clashes: Vec<&str> = conflicts(run, &overrides).iter().map(|c| c.id).collect();
        assert_eq!(clashes, vec!["query.save"]);
        assert!(
            conflicts(save, &overrides)
                .iter()
                .any(|c| c.id == "query.run")
        );
        // designer.save also uses Ctrl/⌘+S, but in the designer's context.
        assert!(conflicts(designer_save, &overrides).is_empty());
    }
}
