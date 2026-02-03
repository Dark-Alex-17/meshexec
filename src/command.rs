use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{Display, Formatter};

use crate::config::{Command, Flag};

#[derive(Debug)]
pub enum AliasResult {
    Command {
        command: String,
        env: HashMap<String, String>,
    },
    HelpText(String),
}

#[derive(Debug)]
pub enum AliasError {
    UnknownAlias(String),
    MissingRequiredArg(String),
    MissingRequiredFlag(String),
    MissingFlagValue(String),
    UnknownFlag(String),
    TooManyArgs { expected: usize },
}

impl Display for AliasError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AliasError::UnknownAlias(name) => write!(f, "Unknown command: {name}"),
            AliasError::MissingRequiredArg(name) => write!(f, "Missing required argument: {name}"),
            AliasError::MissingRequiredFlag(name) => write!(f, "Missing required flag: {name}"),
            AliasError::MissingFlagValue(name) => write!(f, "Flag {name} requires a value"),
            AliasError::UnknownFlag(name) => write!(f, "Unknown flag: {name}"),
            AliasError::TooManyArgs { expected } => {
                write!(f, "Too many arguments (expected {expected})")
            }
        }
    }
}

pub fn resolve_alias(message: &str, commands: &[Command]) -> Result<AliasResult> {
    let rest = &message[1..];

    if rest == "help" {
        return Ok(AliasResult::HelpText(format_help_listing(commands, "!")));
    }

    resolve_from(rest, commands, "!")
}

fn resolve_from(input: &str, commands: &[Command], prefix: &str) -> Result<AliasResult> {
    let mut sorted: Vec<&Command> = commands.iter().collect();
    sorted.sort_by(|a, b| b.name.len().cmp(&a.name.len()));

    let (cmd, args_str) = sorted
        .iter()
        .find_map(|c| match_command(input, c))
        .ok_or_else(|| {
            let first_word = input.split_whitespace().next().unwrap_or(input);
            anyhow!(AliasError::UnknownAlias(format!("{prefix}{first_word}")))
        })?;

    let is_group = !cmd.commands.is_empty();
    let new_prefix = format!("{prefix}{} ", cmd.name);

    if is_group {
        if args_str.is_empty() {
            return Ok(AliasResult::HelpText(format_group_help(cmd, prefix)));
        }

        let trimmed = args_str.trim();
        if trimmed == "--help" || trimmed == "-h" {
            return Ok(AliasResult::HelpText(format_group_help(cmd, prefix)));
        }

        return resolve_from(args_str, &cmd.commands, &new_prefix);
    }

    let tokens: Vec<&str> = if args_str.is_empty() {
        Vec::new()
    } else {
        args_str.split_whitespace().collect()
    };

    if tokens.iter().any(|t| *t == "-h" || *t == "--help") {
        return Ok(AliasResult::HelpText(format_command_help(cmd, prefix)));
    }

    let env = parse_tokens(&tokens, cmd)?;

    Ok(AliasResult::Command {
        command: cmd.command.clone(),
        env,
    })
}

fn match_command<'a>(input: &'a str, cmd: &'a Command) -> Option<(&'a Command, &'a str)> {
    if input == cmd.name {
        Some((cmd, ""))
    } else if input.starts_with(&cmd.name) && input.as_bytes().get(cmd.name.len()) == Some(&b' ') {
        Some((cmd, input[cmd.name.len()..].trim()))
    } else {
        None
    }
}

fn parse_tokens(tokens: &[&str], cmd: &Command) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    let mut positional_idx = 0;
    let mut i = 0;

    while i < tokens.len() {
        let token = tokens[i];

        if token.starts_with('-') {
            let flag = find_flag(token, &cmd.flags)
                .ok_or_else(|| anyhow!(AliasError::UnknownFlag(token.to_string())))?;

            if let Some(arg_name) = flag.arg.as_ref() {
                i += 1;
                if flag.greedy {
                    if i >= tokens.len() {
                        return Err(anyhow!(AliasError::MissingFlagValue(flag.long.clone())));
                    }
                    let value = tokens[i..].join(" ");
                    vars.insert(arg_name.clone(), value);
                    break;
                }
                let value = tokens
                    .get(i)
                    .ok_or_else(|| anyhow!(AliasError::MissingFlagValue(flag.long.clone())))?;
                vars.insert(arg_name.clone(), value.to_string());
            } else {
                let var_name = flag.long.trim_start_matches('-').replace('-', "_");
                vars.insert(var_name, "true".to_string());
            }
        } else {
            if positional_idx >= cmd.args.len() {
                return Err(anyhow!(AliasError::TooManyArgs {
                    expected: cmd.args.len(),
                }));
            }
            let arg = &cmd.args[positional_idx];
            let var_name = arg.name.replace('-', "_");
            if arg.greedy {
                let value = tokens[i..].join(" ");
                vars.insert(var_name, value);
                positional_idx = cmd.args.len();
                break;
            }
            vars.insert(var_name, token.to_string());
            positional_idx += 1;
        }

        i += 1;
    }

    for arg in cmd.args.iter().skip(positional_idx) {
        let var_name = arg.name.replace('-', "_");
        if let Some(default) = arg.default.as_ref() {
            vars.insert(var_name, default.clone());
        } else {
            return Err(anyhow!(AliasError::MissingRequiredArg(arg.name.clone())));
        }
    }

    for flag in &cmd.flags {
        let var_name = if let Some(arg_name) = flag.arg.as_ref() {
            arg_name.clone()
        } else {
            flag.long.trim_start_matches('-').replace('-', "_")
        };

        if let Entry::Vacant(e) = vars.entry(var_name) {
            if let Some(default) = flag.default.as_ref() {
                e.insert(default.clone());
            } else if flag.required {
                return Err(anyhow!(AliasError::MissingRequiredFlag(flag.long.clone())));
            }
        }
    }

    Ok(vars)
}

fn find_flag<'a>(token: &str, flags: &'a [Flag]) -> Option<&'a Flag> {
    flags
        .iter()
        .find(|f| f.long == token || f.short.as_deref() == Some(token))
}

fn format_help_listing(commands: &[Command], prefix: &str) -> String {
    let mut output = String::from("Commands:\n");
    for cmd in commands {
        output.push_str(&format!("  {prefix}{}", cmd.name));
        if !cmd.help.is_empty() {
            output.push_str(&format!(" - {}", cmd.help));
        }
        output.push('\n');
    }
    output.push_str(&format!("\nSend {prefix}<command> --help for details."));
    output
}

fn format_group_help(cmd: &Command, prefix: &str) -> String {
    let mut output = format!("{prefix}{}", cmd.name);
    if !cmd.help.is_empty() {
        output.push_str(&format!(" - {}", cmd.help));
    }
    output.push('\n');

    let sub_prefix = format!("{prefix}{} ", cmd.name);
    output.push_str("\nSubcommands:\n");
    for subcommand in &cmd.commands {
        output.push_str(&format!("  {sub_prefix}{}", subcommand.name));
        if !subcommand.help.is_empty() {
            output.push_str(&format!(" - {}", subcommand.help));
        }
        output.push('\n');
    }
    output.push_str(&format!("\nSend {sub_prefix}<command> --help for details."));
    output
}

fn format_command_help(cmd: &Command, prefix: &str) -> String {
    let mut output = format!("{prefix}{}", cmd.name);
    if !cmd.help.is_empty() {
        output.push_str(&format!(" - {}", cmd.help));
    }
    output.push('\n');

    if !cmd.args.is_empty() {
        output.push_str("\nArgs:\n");
        for arg in &cmd.args {
            if arg.greedy {
                output.push_str(&format!("  <{}...>", arg.name));
            } else {
                output.push_str(&format!("  <{}>", arg.name));
            }
            if !arg.help.is_empty() {
                output.push_str(&format!(" - {}", arg.help));
            }
            if let Some(ref default) = arg.default {
                output.push_str(&format!(" (default: {default})"));
            }
            output.push('\n');
        }
    }

    if !cmd.flags.is_empty() {
        output.push_str("\nFlags:\n");
        for flag in &cmd.flags {
            output.push_str("  ");
            if let Some(ref short) = flag.short {
                output.push_str(&format!("{short}, "));
            }
            output.push_str(&flag.long);
            if let Some(ref arg_name) = flag.arg {
                if flag.greedy {
                    output.push_str(&format!(" <{arg_name}...>"));
                } else {
                    output.push_str(&format!(" <{arg_name}>"));
                }
            }
            if let Some(ref help) = flag.help {
                output.push_str(&format!(" - {help}"));
            }
            if flag.required {
                output.push_str(" (required)");
            }
            if let Some(ref default) = flag.default {
                output.push_str(&format!(" (default: {default})"));
            }
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Arg, Command, Flag};

    fn leaf(name: &str, command: &str) -> Command {
        Command {
            name: name.to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: command.to_string(),
            commands: vec![],
        }
    }

    fn leaf_with_help(name: &str, command: &str, help: &str) -> Command {
        Command {
            name: name.to_string(),
            help: help.to_string(),
            args: vec![],
            flags: vec![],
            command: command.to_string(),
            commands: vec![],
        }
    }

    fn arg(name: &str) -> Arg {
        Arg {
            name: name.to_string(),
            help: String::new(),
            default: None,
            greedy: false,
        }
    }

    fn arg_with_default(name: &str, default: &str) -> Arg {
        Arg {
            name: name.to_string(),
            help: String::new(),
            default: Some(default.to_string()),
            greedy: false,
        }
    }

    fn greedy_arg(name: &str) -> Arg {
        Arg {
            name: name.to_string(),
            help: String::new(),
            default: None,
            greedy: true,
        }
    }

    fn bool_flag(long: &str, short: Option<&str>) -> Flag {
        Flag {
            long: long.to_string(),
            short: short.map(|s| s.to_string()),
            help: None,
            arg: None,
            required: false,
            default: None,
            greedy: false,
        }
    }

    fn value_flag(long: &str, short: Option<&str>, arg_name: &str) -> Flag {
        Flag {
            long: long.to_string(),
            short: short.map(|s| s.to_string()),
            help: None,
            arg: Some(arg_name.to_string()),
            required: false,
            default: None,
            greedy: false,
        }
    }

    fn unwrap_command(result: AliasResult) -> (String, HashMap<String, String>) {
        match result {
            AliasResult::Command { command, env } => (command, env),
            AliasResult::HelpText(t) => panic!("expected Command, got HelpText: {t}"),
        }
    }

    fn unwrap_help(result: AliasResult) -> String {
        match result {
            AliasResult::HelpText(t) => t,
            AliasResult::Command { command, .. } => {
                panic!("expected HelpText, got Command: {command}")
            }
        }
    }

    #[test]
    fn help_returns_command_listing() {
        let cmds = vec![leaf("ping", "do-ping")];
        let text = unwrap_help(resolve_alias("!help", &cmds).unwrap());
        assert!(text.contains("Commands:"));
    }

    #[test]
    fn unknown_command_returns_error() {
        let cmds = vec![leaf("ping", "do-ping")];
        let err = resolve_alias("!unknown", &cmds).unwrap_err();
        assert!(err.to_string().contains("Unknown command: !unknown"));
    }

    #[test]
    fn leaf_no_args_resolves() {
        let cmds = vec![leaf("ping", "do-ping")];
        let (cmd, env) = unwrap_command(resolve_alias("!ping", &cmds).unwrap());
        assert_eq!(cmd, "do-ping");
        assert!(env.is_empty());
    }

    #[test]
    fn leaf_with_one_positional_arg() {
        let mut c = leaf("greet", "say-hello");
        c.args.push(arg("name"));
        let cmds = vec![c];
        let (cmd, env) = unwrap_command(resolve_alias("!greet Alice", &cmds).unwrap());
        assert_eq!(cmd, "say-hello");
        assert_eq!(env.get("name").unwrap(), "Alice");
    }

    #[test]
    fn leaf_dash_dash_help() {
        let cmds = vec![leaf("ping", "do-ping")];
        let text = unwrap_help(resolve_alias("!ping --help", &cmds).unwrap());
        assert!(text.contains("!ping"));
    }

    #[test]
    fn leaf_dash_h() {
        let cmds = vec![leaf("ping", "do-ping")];
        let text = unwrap_help(resolve_alias("!ping -h", &cmds).unwrap());
        assert!(text.contains("!ping"));
    }

    #[test]
    fn help_flag_takes_priority_over_args() {
        let mut c = leaf("greet", "say-hello");
        c.args.push(arg("name"));
        let cmds = vec![c];
        let text = unwrap_help(resolve_alias("!greet Alice --help", &cmds).unwrap());
        assert!(text.contains("!greet"));
    }

    #[test]
    fn group_no_subcommand_returns_help() {
        let group = Command {
            name: "deploy".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![leaf("prod", "deploy-prod")],
        };
        let cmds = vec![group];
        let text = unwrap_help(resolve_alias("!deploy", &cmds).unwrap());
        assert!(text.contains("Subcommands:"));
    }

    #[test]
    fn group_dash_dash_help() {
        let group = Command {
            name: "deploy".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![leaf("prod", "deploy-prod")],
        };
        let cmds = vec![group];
        let text = unwrap_help(resolve_alias("!deploy --help", &cmds).unwrap());
        assert!(text.contains("Subcommands:"));
    }

    #[test]
    fn group_resolves_subcommand() {
        let group = Command {
            name: "deploy".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![leaf("prod", "deploy-prod")],
        };
        let cmds = vec![group];
        let (cmd, _) = unwrap_command(resolve_alias("!deploy prod", &cmds).unwrap());
        assert_eq!(cmd, "deploy-prod");
    }

    #[test]
    fn group_unknown_subcommand() {
        let group = Command {
            name: "deploy".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![leaf("prod", "deploy-prod")],
        };
        let cmds = vec![group];
        let err = resolve_alias("!deploy staging", &cmds).unwrap_err();
        assert!(err.to_string().contains("Unknown command"));
    }

    #[test]
    fn nested_group_resolution() {
        let inner = Command {
            name: "b".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![leaf("c", "run-c")],
        };
        let outer = Command {
            name: "a".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![inner],
        };
        let cmds = vec![outer];
        let (cmd, _) = unwrap_command(resolve_alias("!a b c", &cmds).unwrap());
        assert_eq!(cmd, "run-c");
    }

    #[test]
    fn missing_required_arg() {
        let mut c = leaf("greet", "say-hello");
        c.args.push(arg("name"));
        let cmds = vec![c];
        let err = resolve_alias("!greet", &cmds).unwrap_err();
        assert!(err.to_string().contains("Missing required argument"));
    }

    #[test]
    fn arg_default_used_when_not_provided() {
        let mut c = leaf("greet", "say-hello");
        c.args.push(arg_with_default("name", "World"));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!greet", &cmds).unwrap());
        assert_eq!(env.get("name").unwrap(), "World");
    }

    #[test]
    fn too_many_positional_args() {
        let mut c = leaf("greet", "say-hello");
        c.args.push(arg("name"));
        let cmds = vec![c];
        let err = resolve_alias("!greet Alice Bob", &cmds).unwrap_err();
        assert!(err.to_string().contains("Too many arguments"));
    }

    #[test]
    fn greedy_arg_consumes_remaining_tokens() {
        let mut c = leaf("echo", "run-echo");
        c.args.push(greedy_arg("message"));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!echo hello world foo", &cmds).unwrap());
        assert_eq!(env.get("message").unwrap(), "hello world foo");
    }

    #[test]
    fn arg_name_hyphens_become_underscores() {
        let mut c = leaf("cmd", "run-cmd");
        c.args.push(arg("my-arg"));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd value", &cmds).unwrap());
        assert!(env.contains_key("my_arg"));
        assert_eq!(env.get("my_arg").unwrap(), "value");
    }

    #[test]
    fn boolean_long_flag() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(bool_flag("--verbose", None));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd --verbose", &cmds).unwrap());
        assert_eq!(env.get("verbose").unwrap(), "true");
    }

    #[test]
    fn boolean_short_flag() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(bool_flag("--verbose", Some("-v")));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd -v", &cmds).unwrap());
        assert_eq!(env.get("verbose").unwrap(), "true");
    }

    #[test]
    fn flag_with_value() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(value_flag("--output", Some("-o"), "path"));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd --output /tmp", &cmds).unwrap());
        assert_eq!(env.get("path").unwrap(), "/tmp");
    }

    #[test]
    fn unknown_flag_errors() {
        let cmds = vec![leaf("cmd", "run-cmd")];
        let err = resolve_alias("!cmd --nope", &cmds).unwrap_err();
        assert!(err.to_string().contains("Unknown flag"));
    }

    #[test]
    fn flag_missing_value() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(value_flag("--output", None, "path"));
        let cmds = vec![c];
        let err = resolve_alias("!cmd --output", &cmds).unwrap_err();
        assert!(err.to_string().contains("requires a value"));
    }

    #[test]
    fn required_flag_not_provided() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(Flag {
            long: "--env".to_string(),
            short: None,
            help: None,
            arg: Some("env_name".to_string()),
            required: true,
            default: None,
            greedy: false,
        });
        let cmds = vec![c];
        let err = resolve_alias("!cmd", &cmds).unwrap_err();
        assert!(err.to_string().contains("Missing required flag"));
    }

    #[test]
    fn flag_default_used_when_not_provided() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(Flag {
            long: "--env".to_string(),
            short: None,
            help: None,
            arg: Some("env_name".to_string()),
            required: false,
            default: Some("production".to_string()),
            greedy: false,
        });
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd", &cmds).unwrap());
        assert_eq!(env.get("env_name").unwrap(), "production");
    }

    #[test]
    fn greedy_flag_consumes_remaining() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(Flag {
            long: "--message".to_string(),
            short: None,
            help: None,
            arg: Some("msg".to_string()),
            required: false,
            default: None,
            greedy: true,
        });
        let cmds = vec![c];
        let (_, env) =
            unwrap_command(resolve_alias("!cmd --message hello world foo", &cmds).unwrap());
        assert_eq!(env.get("msg").unwrap(), "hello world foo");
    }

    #[test]
    fn flag_long_hyphens_become_underscores() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(bool_flag("--dry-run", None));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd --dry-run", &cmds).unwrap());
        assert_eq!(env.get("dry_run").unwrap(), "true");
    }

    #[test]
    fn help_listing_includes_names_and_help() {
        let cmds = vec![leaf_with_help("ping", "do-ping", "Check connectivity")];
        let text = unwrap_help(resolve_alias("!help", &cmds).unwrap());
        assert!(text.contains("ping"));
        assert!(text.contains("Check connectivity"));
    }

    #[test]
    fn help_listing_includes_footer() {
        let cmds = vec![leaf("ping", "do-ping")];
        let text = unwrap_help(resolve_alias("!help", &cmds).unwrap());
        assert!(text.contains("Send !<command> --help for details."));
    }

    #[test]
    fn group_help_shows_subcommands() {
        let group = Command {
            name: "deploy".to_string(),
            help: String::new(),
            args: vec![],
            flags: vec![],
            command: String::new(),
            commands: vec![
                leaf_with_help("prod", "deploy-prod", "Production deploy"),
                leaf("staging", "deploy-staging"),
            ],
        };
        let cmds = vec![group];
        let text = unwrap_help(resolve_alias("!deploy", &cmds).unwrap());
        assert!(text.contains("prod"));
        assert!(text.contains("Production deploy"));
        assert!(text.contains("staging"));
    }

    #[test]
    fn command_help_shows_args_and_flags() {
        let mut c = leaf_with_help("greet", "say-hello", "Greet someone");
        c.args.push(Arg {
            name: "name".to_string(),
            help: "Who to greet".to_string(),
            default: None,
            greedy: false,
        });
        c.flags.push(Flag {
            long: "--loud".to_string(),
            short: Some("-l".to_string()),
            help: Some("Shout it".to_string()),
            arg: None,
            required: false,
            default: None,
            greedy: false,
        });
        let cmds = vec![c];
        let text = unwrap_help(resolve_alias("!greet --help", &cmds).unwrap());
        assert!(text.contains("<name>"));
        assert!(text.contains("Who to greet"));
        assert!(text.contains("--loud"));
        assert!(text.contains("-l"));
        assert!(text.contains("Shout it"));
    }

    #[test]
    fn command_help_greedy_arg_notation() {
        let mut c = leaf("echo", "run-echo");
        c.args.push(greedy_arg("words"));
        let cmds = vec![c];
        let text = unwrap_help(resolve_alias("!echo --help", &cmds).unwrap());
        assert!(text.contains("<words...>"));
    }

    #[test]
    fn command_help_required_flag() {
        let mut c = leaf("cmd", "run-cmd");
        c.flags.push(Flag {
            long: "--env".to_string(),
            short: None,
            help: None,
            arg: Some("env_name".to_string()),
            required: true,
            default: None,
            greedy: false,
        });
        let cmds = vec![c];
        let text = unwrap_help(resolve_alias("!cmd --help", &cmds).unwrap());
        assert!(text.contains("(required)"));
    }

    #[test]
    fn command_help_default_values() {
        let mut c = leaf("cmd", "run-cmd");
        c.args.push(arg_with_default("target", "main"));
        c.flags.push(Flag {
            long: "--env".to_string(),
            short: None,
            help: None,
            arg: Some("env_name".to_string()),
            required: false,
            default: Some("dev".to_string()),
            greedy: false,
        });
        let cmds = vec![c];
        let text = unwrap_help(resolve_alias("!cmd --help", &cmds).unwrap());
        assert!(text.contains("(default: main)"));
        assert!(text.contains("(default: dev)"));
    }

    #[test]
    fn exact_match_resolves() {
        let cmds = vec![leaf("foo", "run-foo")];
        let (cmd, _) = unwrap_command(resolve_alias("!foo", &cmds).unwrap());
        assert_eq!(cmd, "run-foo");
    }

    #[test]
    fn prefix_match_with_space_resolves() {
        let mut c = leaf("foo", "run-foo");
        c.args.push(arg_with_default("x", "default"));
        let cmds = vec![c];
        let (cmd, _) = unwrap_command(resolve_alias("!foo bar", &cmds).unwrap());
        assert_eq!(cmd, "run-foo");
    }

    #[test]
    fn prefix_match_without_space_does_not_resolve() {
        let cmds = vec![leaf("foo", "run-foo")];
        let err = resolve_alias("!foobar", &cmds).unwrap_err();
        assert!(err.to_string().contains("Unknown command: !foobar"));
    }

    #[test]
    fn no_match_returns_error() {
        let cmds = vec![leaf("ping", "do-ping")];
        let err = resolve_alias("!zzz", &cmds).unwrap_err();
        assert!(err.to_string().contains("Unknown command: !zzz"));
    }

    #[test]
    fn alias_error_unknown_alias_display() {
        let e = AliasError::UnknownAlias("!bad".to_string());
        assert_eq!(e.to_string(), "Unknown command: !bad");
    }

    #[test]
    fn alias_error_missing_required_arg_display() {
        let e = AliasError::MissingRequiredArg("name".to_string());
        assert_eq!(e.to_string(), "Missing required argument: name");
    }

    #[test]
    fn alias_error_missing_required_flag_display() {
        let e = AliasError::MissingRequiredFlag("--env".to_string());
        assert_eq!(e.to_string(), "Missing required flag: --env");
    }

    #[test]
    fn alias_error_missing_flag_value_display() {
        let e = AliasError::MissingFlagValue("--output".to_string());
        assert_eq!(e.to_string(), "Flag --output requires a value");
    }

    #[test]
    fn alias_error_unknown_flag_display() {
        let e = AliasError::UnknownFlag("--nope".to_string());
        assert_eq!(e.to_string(), "Unknown flag: --nope");
    }

    #[test]
    fn alias_error_too_many_args_display() {
        let e = AliasError::TooManyArgs { expected: 2 };
        assert_eq!(e.to_string(), "Too many arguments (expected 2)");
    }

    #[test]
    fn positional_then_flag() {
        let mut c = leaf("cmd", "run-cmd");
        c.args.push(arg("target"));
        c.flags.push(value_flag("--env", None, "env_name"));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd prod --env staging", &cmds).unwrap());
        assert_eq!(env.get("target").unwrap(), "prod");
        assert_eq!(env.get("env_name").unwrap(), "staging");
    }

    #[test]
    fn flag_then_positional() {
        let mut c = leaf("cmd", "run-cmd");
        c.args.push(arg("target"));
        c.flags.push(value_flag("--env", None, "env_name"));
        let cmds = vec![c];
        let (_, env) = unwrap_command(resolve_alias("!cmd --env staging prod", &cmds).unwrap());
        assert_eq!(env.get("target").unwrap(), "prod");
        assert_eq!(env.get("env_name").unwrap(), "staging");
    }

    #[test]
    fn interleaved_flags_and_args() {
        let mut c = leaf("cmd", "run-cmd");
        c.args.push(arg("src"));
        c.args.push(arg("dst"));
        c.flags.push(bool_flag("--verbose", Some("-v")));
        c.flags.push(value_flag("--mode", None, "mode"));
        let cmds = vec![c];
        let (_, env) =
            unwrap_command(resolve_alias("!cmd -v origin --mode fast dest", &cmds).unwrap());
        assert_eq!(env.get("verbose").unwrap(), "true");
        assert_eq!(env.get("src").unwrap(), "origin");
        assert_eq!(env.get("mode").unwrap(), "fast");
        assert_eq!(env.get("dst").unwrap(), "dest");
    }
}
