//! Parsing the command text typed into a query tab, and the safety lists the driver applies to it.
//!
//! Redis has no SQL grammar: a "script" is one command per line, each split into arguments with
//! shell-like quoting. `split_script` is what [`rustgrid_core::Connection::split_statements`]
//! returns, and `parse_args` feeds the Redis protocol.

/// One parsed command: the command name plus its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandLine {
    pub name: String,
    pub args: Vec<String>,
}

impl CommandLine {
    /// The command name lowercased, for the safety lists and version gates.
    pub fn name_lower(&self) -> String {
        self.name.to_ascii_lowercase()
    }
}

/// Splits a query script into commands: one per line, ignoring blank lines and `#` comments.
///
/// Redis commands cannot span lines, so a line-oriented split is the whole grammar. Statements
/// keep their trailing `\r` out (the editor may be configured with CRLF).
pub(crate) fn split_script(script: &str) -> Vec<String> {
    script
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// Parses one command line into its name and arguments.
///
/// Quoting follows the redis-cli convention: whitespace separates arguments, `'…'` and `"…"` group
/// a value that contains spaces, and a backslash escapes the next character (inside double quotes
/// it also accepts `\n`, `\r`, `\t` and `\xHH`).
pub(crate) fn parse_args(line: &str) -> Result<CommandLine, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut has_current = false;
    let mut chars = line.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            ' ' | '\t' if !has_current => continue,
            ' ' | '\t' => {
                args.push(std::mem::take(&mut current));
                has_current = false;
            }
            '\'' | '"' => {
                has_current = true;
                let quote = character;
                let mut closed = false;
                while let Some(inner) = chars.next() {
                    if inner == quote {
                        closed = true;
                        break;
                    }
                    if inner == '\\' && quote == '"' {
                        push_escape(&mut current, &mut chars)?;
                    } else {
                        current.push(inner);
                    }
                }
                if !closed {
                    return Err(format!("unterminated {quote} quote"));
                }
                // A closing quote must be followed by a separator or the end of the line.
                if let Some(next) = chars.peek()
                    && next != &' '
                    && next != &'\t'
                {
                    return Err("unexpected character after a closing quote".to_string());
                }
            }
            '\\' => {
                has_current = true;
                push_escape(&mut current, &mut chars)?;
            }
            _ => {
                has_current = true;
                current.push(character);
            }
        }
    }
    if has_current {
        args.push(current);
    }
    let Some(name) = args.first().cloned() else {
        return Err("empty command".to_string());
    };
    Ok(CommandLine {
        name,
        args: args.into_iter().skip(1).collect(),
    })
}

fn push_escape(
    current: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(), String> {
    let Some(escaped) = chars.next() else {
        return Err("trailing backslash".to_string());
    };
    match escaped {
        'n' => current.push('\n'),
        'r' => current.push('\r'),
        't' => current.push('\t'),
        'x' => {
            let high = chars.next().ok_or("incomplete \\x escape")?;
            let low = chars.next().ok_or("incomplete \\x escape")?;
            let hex = format!("{high}{low}");
            let byte = u8::from_str_radix(&hex, 16).map_err(|_| "invalid \\x escape")?;
            current.push(byte as char);
        }
        other => current.push(other),
    }
    Ok(())
}

/// Commands that would block the shared connection (their reply only arrives when another client
/// acts), so they are refused rather than stalling every other tab until a timeout.
pub(crate) fn blocking_command(name: &str) -> bool {
    matches!(
        name,
        "blpop"
            | "brpop"
            | "blmove"
            | "brpoplpush"
            | "blmpop"
            | "bzpopmin"
            | "bzpopmax"
            | "bzmpop"
            | "wait"
            | "waitaof"
            | "monitor"
            | "subscribe"
            | "psubscribe"
            | "ssubscribe"
            | "sync"
            | "psync"
    )
}

/// Commands that change connection state, which the pool would then leak into other tabs.
pub(crate) fn stateful_command(name: &str) -> bool {
    matches!(
        name,
        "select"
            | "auth"
            | "hello"
            | "reset"
            | "quit"
            | "readonly"
            | "readwrite"
            | "multi"
            | "exec"
            | "discard"
            | "watch"
            | "unwatch"
    )
}

/// `CLIENT` sub-commands that change how the connection reports replies; running them on the
/// shared connection would break every other tab.
pub(crate) fn stateful_client_subcommand(subcommand: &str) -> bool {
    matches!(
        subcommand,
        "reply" | "tracking" | "caching" | "pause" | "unpause" | "no-evict" | "no-touch"
    )
}

/// Commands that destroy or reconfigure the server; refused in the query editor.
pub(crate) fn destructive_command(name: &str) -> bool {
    matches!(
        name,
        "shutdown" | "debug" | "failover" | "replicaof" | "slaveof" | "swapdb"
    )
}

/// Whether a command writes. Used by the connection's read-only safety mode.
///
/// The list is deliberately broad rather than clever: a read-only connection is a guard rail, and
/// letting an unknown command through would defeat it. Commands whose sub-command decides
/// (`CONFIG GET` vs `CONFIG SET`, `CLIENT ID` vs `CLIENT KILL`, …) are checked explicitly.
pub(crate) fn write_command(name: &str, args: &[String]) -> bool {
    const WRITE: &[&str] = &[
        "append",
        "bitop",
        "copy",
        "decr",
        "decrby",
        "del",
        "discard",
        "exec",
        "expire",
        "expireat",
        "flushall",
        "flushdb",
        "geoadd",
        "georadius",
        "geosearchstore",
        "getdel",
        "getex",
        "getset",
        "hdel",
        "hincrby",
        "hincrbyfloat",
        "hmset",
        "hset",
        "hsetnx",
        "incr",
        "incrby",
        "incrbyfloat",
        "linsert",
        "lmove",
        "lmpop",
        "lpop",
        "lpush",
        "lpushx",
        "lrem",
        "lset",
        "ltrim",
        "migrate",
        "move",
        "mset",
        "msetnx",
        "multi",
        "persist",
        "pexpire",
        "pexpireat",
        "pfadd",
        "pfmerge",
        "psetex",
        "publish",
        "rename",
        "renamenx",
        "restore",
        "rpop",
        "rpoplpush",
        "rpush",
        "rpushx",
        "sadd",
        "sdiffstore",
        "set",
        "setbit",
        "setex",
        "setnx",
        "setrange",
        "sinterstore",
        "smove",
        "sort",
        "spop",
        "spublish",
        "srem",
        "sunionstore",
        "unlink",
        "xack",
        "xadd",
        "xautoclaim",
        "xclaim",
        "xdel",
        "xgroup",
        "xsetid",
        "xtrim",
        "zadd",
        "zincrby",
        "zpopmax",
        "zpopmin",
        "zrem",
        "zremrangebylex",
        "zremrangebyrank",
        "zremrangebyscore",
        "zunionstore",
        "zinterstore",
        "zdiffstore",
        "zrangestore",
    ];
    if WRITE.contains(&name) {
        return true;
    }
    let sub = || {
        args.first()
            .map(|argument| argument.to_ascii_lowercase())
            .unwrap_or_default()
    };
    match name {
        "eval" | "evalsha" | "eval_ro" | "evalsha_ro" | "fcall" | "fcall_ro" => true,
        "config" => sub() != "get" && sub() != "help",
        "client" => matches!(
            sub().as_str(),
            "kill"
                | "pause"
                | "unpause"
                | "no-evict"
                | "no-touch"
                | "setname"
                | "setinfo"
                | "tracking"
                | "caching"
                | "reply"
        ),
        "acl" => matches!(
            sub().as_str(),
            "setuser" | "deluser" | "load" | "save" | "genpass"
        ),
        "script" => matches!(sub().as_str(), "load" | "flush"),
        "function" => matches!(sub().as_str(), "load" | "delete" | "flush" | "restore"),
        "module" => sub() != "list",
        _ if name.starts_with("json.") => !matches!(
            name.trim_start_matches("json."),
            "get" | "mget" | "type" | "strlen" | "arrlen" | "objlen" | "objkeys" | "debug"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_script_one_command_per_line() {
        let script = "PING\n\n# a note\r\nGET a\r\n  HGETALL h  \r\n";
        assert_eq!(split_script(script), vec!["PING", "GET a", "HGETALL h"]);
    }

    #[test]
    fn tokenizes_plain_and_quoted_arguments() {
        assert_eq!(
            parse_args("SET greeting \"hello world\"").unwrap(),
            CommandLine {
                name: "SET".to_string(),
                args: vec!["greeting".to_string(), "hello world".to_string()]
            }
        );
        assert_eq!(
            parse_args("  SET  k  'a b'  ").unwrap().args,
            vec!["k".to_string(), "a b".to_string()]
        );
        assert_eq!(
            parse_args(r#"SET k "a\nb""#).unwrap().args,
            vec!["k".to_string(), "a\nb".to_string()]
        );
        assert_eq!(
            parse_args(r"SET k a\ b").unwrap().args,
            vec!["k".to_string(), "a b".to_string()]
        );
        assert!(parse_args("SET \"unterminated").is_err());
        assert!(parse_args("   ").is_err());
    }

    #[test]
    fn flags_blocking_and_write_commands() {
        assert!(blocking_command("subscribe"));
        assert!(blocking_command("blpop"));
        assert!(!blocking_command("get"));

        assert!(write_command("set", &[]));
        assert!(write_command("hset", &[]));
        assert!(write_command("json.set", &["key".into()]));
        assert!(write_command("eval", &[]));
        assert!(!write_command("get", &[]));
        assert!(!write_command("hgetall", &[]));
        assert!(!write_command("json.get", &["key".into()]));
        assert!(!write_command("config", &["get".into(), "*".into()]));
        assert!(write_command("config", &["set".into()]));
        assert!(!write_command("client", &["id".into()]));
        assert!(write_command("client", &["kill".into()]));
    }
}
