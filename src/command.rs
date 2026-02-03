use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{Display, Formatter};

use crate::config::{Command, Flag};

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
