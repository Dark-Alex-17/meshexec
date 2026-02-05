use anyhow::{Result, anyhow};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

pub trait Validate {
  fn validate(&self) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arg {
  pub name: String,
  pub help: String,
  pub default: Option<String>,
  #[serde(default)]
  pub greedy: bool,
}

impl Validate for Arg {
  fn validate(&self) -> Result<()> {
    if let Some(default_value) = self.default.as_deref()
      && default_value.is_empty()
    {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Default values in arguments cannot be empty: {self:?}"
      ))));
    }

    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flag {
  pub long: String,
  pub short: Option<String>,
  pub help: Option<String>,
  pub arg: Option<String>,
  #[serde(default)]
  pub required: bool,
  pub default: Option<String>,
  #[serde(default)]
  pub greedy: bool,
}

impl Validate for Flag {
  fn validate(&self) -> Result<()> {
    if !Regex::new(r"^--[a-zA-Z0-9-]*$")?.is_match(self.long.trim()) {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Invalid long flag value: {}",
        self.long
      ))));
    }

    if let Some(short) = self.short.as_deref()
      && !Regex::new(r"^-[a-zA-Z0-9]$")?.is_match(short.trim())
    {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Invalid short flag value: {short}"
      ))));
    }

    if self.greedy && self.arg.is_none() {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Greedy flag {} must have an 'arg' field",
        self.long
      ))));
    }

    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
  pub name: String,
  #[serde(default)]
  pub help: String,
  #[serde(default)]
  pub args: Vec<Arg>,
  #[serde(default)]
  pub flags: Vec<Flag>,
  #[serde(default)]
  pub command: String,
  #[serde(default)]
  pub commands: Vec<Command>,
}

impl Validate for Command {
  fn validate(&self) -> Result<()> {
    if self.name.is_empty() {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Command names cannot be empty: {self:?}"
      ))));
    }

    let is_group = !self.commands.is_empty();
    let is_leaf = !self.command.is_empty();

    if is_group && is_leaf {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Command '{}': cannot have both 'command' and 'commands'",
        self.name
      ))));
    }

    if !is_group && !is_leaf {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Command '{}': must have either 'command' or 'commands'",
        self.name
      ))));
    }

    if is_group {
      if !self.args.is_empty() || !self.flags.is_empty() {
        return Err(anyhow!(ConfigError::ValidationError(format!(
          "Command '{}': group commands cannot have args or flags",
          self.name
        ))));
      }
      for subcommand in &self.commands {
        subcommand.validate()?;
      }
      return Ok(());
    }

    for arg in &self.args {
      arg.validate()?;
    }

    for flag in &self.flags {
      flag.validate()?;
    }

    let greedy_arg_count = self.args.iter().filter(|a| a.greedy).count();
    let greedy_flag_count = self.flags.iter().filter(|f| f.greedy).count();
    let total_greedy = greedy_arg_count + greedy_flag_count;

    if total_greedy > 1 {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Command '{}': only one arg or flag can be greedy",
        self.name
      ))));
    }

    if greedy_arg_count == 1 && !self.args.last().is_some_and(|a| a.greedy) {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Command '{}': greedy arg must be the last arg",
        self.name
      ))));
    }

    if greedy_flag_count == 1 && !self.flags.last().is_some_and(|f| f.greedy) {
      return Err(anyhow!(ConfigError::ValidationError(format!(
        "Command '{}': greedy flag must be the last flag",
        self.name
      ))));
    }

    Ok(())
  }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CommandEntry {
  Import { import: String },
  Command(RawCommand),
}

#[derive(Debug, Clone, Deserialize)]
struct RawCommand {
  name: String,
  #[serde(default)]
  help: String,
  #[serde(default)]
  args: Vec<Arg>,
  #[serde(default)]
  flags: Vec<Flag>,
  #[serde(default)]
  command: String,
  #[serde(default)]
  commands: Vec<CommandEntry>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
  device: String,
  channel: u32,
  baud: Option<u32>,
  shell: String,
  #[serde(default)]
  shell_args: Vec<String>,
  max_text_bytes: usize,
  chunk_delay: u64,
  max_content_bytes: usize,
  commands: Vec<CommandEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Config {
  pub device: String,
  pub channel: u32,
  pub baud: Option<u32>,
  pub shell: String,
  pub shell_args: Vec<String>,
  pub max_text_bytes: usize,
  pub chunk_delay: u64,
  pub max_content_bytes: usize,
  pub commands: Vec<Command>,
}

impl Validate for Config {
  fn validate(&self) -> Result<()> {
    if self.commands.is_empty() {
      return Err(anyhow!(ConfigError::ValidationError(
        "At least one command is required to be defined".to_owned()
      )));
    }

    for command in &self.commands {
      command.validate()?
    }

    Ok(())
  }
}

pub struct ConfigLoader {
  base_path: PathBuf,
  loaded_files: HashSet<PathBuf>,
}

impl ConfigLoader {
  pub fn new(base_path: impl AsRef<Path>) -> Self {
    Self {
      base_path: base_path.as_ref().to_path_buf(),
      loaded_files: HashSet::new(),
    }
  }

  pub fn load(&mut self, config_path: impl AsRef<Path>) -> Result<Config> {
    let config_path = self.base_path.join(config_path.as_ref());
    let canonical_path = config_path
      .canonicalize()
      .map_err(|e| ConfigError::FileNotFound(config_path.clone(), e))?;

    if !self.loaded_files.insert(canonical_path.clone()) {
      return Err(anyhow!(ConfigError::CircularImport(canonical_path)));
    }

    let content = fs::read_to_string(&config_path)
      .map_err(|e| ConfigError::FileNotFound(config_path.clone(), e))?;

    let raw: RawConfig = serde_yaml::from_str(&content)
      .map_err(|e| ConfigError::ParseError(config_path.clone(), e))?;

    let commands = self.resolve_commands(&raw.commands, &config_path)?;

    Ok(Config {
      device: raw.device,
      channel: raw.channel,
      baud: raw.baud,
      shell: raw.shell,
      shell_args: raw.shell_args,
      max_text_bytes: raw.max_text_bytes,
      chunk_delay: raw.chunk_delay,
      max_content_bytes: raw.max_content_bytes,
      commands,
    })
  }

  fn resolve_commands(
    &mut self,
    entries: &[CommandEntry],
    current_file: &Path,
  ) -> Result<Vec<Command>> {
    let mut resolved = Vec::new();
    let parent_dir = current_file.parent().unwrap_or(Path::new("."));

    for entry in entries {
      match entry {
        CommandEntry::Import { import } => {
          let import_path = parent_dir.join(import);
          let imported_commands = self.load_command_file(&import_path)?;
          resolved.extend(imported_commands);
        }
        CommandEntry::Command(raw_cmd) => {
          let cmd = self.resolve_command(raw_cmd.clone(), current_file)?;
          resolved.push(cmd);
        }
      }
    }

    Ok(resolved)
  }

  fn load_command_file(&mut self, path: &Path) -> Result<Vec<Command>> {
    let canonical_path = path
      .canonicalize()
      .map_err(|e| ConfigError::FileNotFound(path.to_path_buf(), e))?;

    if !self.loaded_files.insert(canonical_path.clone()) {
      return Err(anyhow!(ConfigError::CircularImport(canonical_path)));
    }

    let content =
      fs::read_to_string(path).map_err(|e| ConfigError::FileNotFound(path.to_path_buf(), e))?;

    if let Ok(raw_cmd) = serde_yaml::from_str::<RawCommand>(&content) {
      let cmd = self.resolve_command(raw_cmd, path)?;
      return Ok(vec![cmd]);
    }

    let entries: Vec<CommandEntry> =
      serde_yaml::from_str(&content).map_err(|e| ConfigError::ParseError(path.to_path_buf(), e))?;

    self.resolve_commands(&entries, path)
  }

  fn resolve_command(&mut self, raw: RawCommand, current_file: &Path) -> Result<Command> {
    let parent_dir = current_file.parent().unwrap_or(Path::new("."));
    let mut resolved_subcommands = Vec::new();

    for entry in raw.commands {
      match entry {
        CommandEntry::Import { import } => {
          let import_path = parent_dir.join(&import);
          let imported = self.load_command_file(&import_path)?;
          resolved_subcommands.extend(imported);
        }
        CommandEntry::Command(sub_raw) => {
          let sub_cmd = self.resolve_command(sub_raw, current_file)?;
          resolved_subcommands.push(sub_cmd);
        }
      }
    }

    Ok(Command {
      name: raw.name,
      help: raw.help,
      args: raw.args,
      flags: raw.flags,
      command: raw.command,
      commands: resolved_subcommands,
    })
  }
}

#[derive(Debug)]
pub enum ConfigError {
  FileNotFound(PathBuf, std::io::Error),
  ParseError(PathBuf, serde_yaml::Error),
  CircularImport(PathBuf),
  ValidationError(String),
  ConfigNotFound(Vec<PathBuf>),
}

impl Display for ConfigError {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ConfigError::FileNotFound(path, e) => {
        write!(f, "Failed to read file '{}': {}", path.display(), e)
      }
      ConfigError::ParseError(path, e) => {
        write!(f, "Failed to parse YAML in '{}': {}", path.display(), e)
      }
      ConfigError::CircularImport(path) => {
        write!(f, "Circular import detected: '{}'", path.display())
      }
      ConfigError::ValidationError(message) => {
        write!(f, "Validation failed: '{message}'")
      }
      ConfigError::ConfigNotFound(paths) => {
        let searched: Vec<_> = paths
          .iter()
          .map(|p| format!("  - {}", p.display()))
          .collect();
        write!(
          f,
          "Config file not found. Searched locations:\n{}",
          searched.join("\n")
        )
      }
    }
  }
}

impl Error for ConfigError {}

pub fn load_config(path: impl AsRef<Path>) -> Result<Config> {
  let yaml_path = path.as_ref().with_extension("yaml");
  let base_yaml_path = yaml_path.parent().unwrap_or(Path::new("."));
  let yaml_file_name = yaml_path.file_name().unwrap_or_default();

  let mut loader = ConfigLoader::new(base_yaml_path);
  let config = match loader.load(yaml_file_name) {
    Ok(config) => Ok(config),
    Err(_) => {
      let yml_path = path.as_ref().with_extension("yml");
      let base_yml_path = yml_path.parent().unwrap_or(Path::new("."));
      let yml_file_name = yml_path.file_name().unwrap_or_default();

      let mut loader = ConfigLoader::new(base_yml_path);
      loader.load(yml_file_name)
    }
  }?;
  config.validate()?;

  Ok(config)
}

pub fn find_config_file() -> Result<PathBuf> {
  let mut searched_paths = Vec::new();

  let current_dir_yaml = PathBuf::from("config.yaml");
  let current_dir_yml = PathBuf::from("config.yml");

  if current_dir_yaml.exists() {
    return Ok(current_dir_yaml);
  }
  searched_paths.push(current_dir_yaml);

  if current_dir_yml.exists() {
    return Ok(current_dir_yml);
  }
  searched_paths.push(current_dir_yml);

  if let Some(config_dir) = dirs_next::config_dir() {
    let xdg_yaml = config_dir.join("meshexec").join("config.yaml");
    let xdg_yml = config_dir.join("meshexec").join("config.yml");

    if xdg_yaml.exists() {
      return Ok(xdg_yaml);
    }
    searched_paths.push(xdg_yaml);

    if xdg_yml.exists() {
      return Ok(xdg_yml);
    }
    searched_paths.push(xdg_yml);
  }

  Err(anyhow!(ConfigError::ConfigNotFound(searched_paths)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use indoc::indoc;
  use std::fs;
  use tempfile::TempDir;

  fn leaf_cmd(name: &str, command: &str) -> Command {
    Command {
      name: name.to_string(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: command.to_string(),
      commands: vec![],
    }
  }

  fn valid_config() -> Config {
    Config {
      device: "/dev/ttyUSB0".into(),
      channel: 1,
      baud: None,
      shell: "bash".into(),
      shell_args: vec!["-lc".into()],
      max_text_bytes: 200,
      chunk_delay: 10000,
      max_content_bytes: 180,
      commands: vec![leaf_cmd("test", "echo hello")],
    }
  }

  fn valid_config_yaml() -> String {
    indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - name: test
                command: echo hello
        "}
    .to_string()
  }

  #[test]
  fn arg_valid_no_default() {
    let arg = Arg {
      name: "file".into(),
      help: "path to file".into(),
      default: None,
      greedy: false,
    };
    assert!(arg.validate().is_ok());
  }

  #[test]
  fn arg_valid_non_empty_default() {
    let arg = Arg {
      name: "file".into(),
      help: "path to file".into(),
      default: Some("default.txt".into()),
      greedy: false,
    };
    assert!(arg.validate().is_ok());
  }

  #[test]
  fn arg_empty_default_fails() {
    let arg = Arg {
      name: "file".into(),
      help: "path to file".into(),
      default: Some(String::new()),
      greedy: false,
    };
    let err = arg.validate().unwrap_err().to_string();
    assert!(
      err.contains("Default values in arguments cannot be empty"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn flag_valid_long_only() {
    let flag = Flag {
      long: "--foo".into(),
      short: None,
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: false,
    };
    assert!(flag.validate().is_ok());
  }

  #[test]
  fn flag_valid_long_and_short() {
    let flag = Flag {
      long: "--foo".into(),
      short: Some("-f".into()),
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: false,
    };
    assert!(flag.validate().is_ok());
  }

  #[test]
  fn flag_invalid_long_no_dashes() {
    let flag = Flag {
      long: "foo".into(),
      short: None,
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: false,
    };
    let err = flag.validate().unwrap_err().to_string();
    assert!(err.contains("Invalid long flag"), "unexpected error: {err}");
  }

  #[test]
  fn flag_invalid_long_special_chars() {
    let flag = Flag {
      long: "--foo@bar".into(),
      short: None,
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: false,
    };
    let err = flag.validate().unwrap_err().to_string();
    assert!(err.contains("Invalid long flag"), "unexpected error: {err}");
  }

  #[test]
  fn flag_invalid_short_too_long() {
    let flag = Flag {
      long: "--foo".into(),
      short: Some("-foo".into()),
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: false,
    };
    let err = flag.validate().unwrap_err().to_string();
    assert!(
      err.contains("Invalid short flag"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn flag_invalid_short_double_dash() {
    let flag = Flag {
      long: "--foo".into(),
      short: Some("--f".into()),
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: false,
    };
    let err = flag.validate().unwrap_err().to_string();
    assert!(
      err.contains("Invalid short flag"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn flag_greedy_without_arg_fails() {
    let flag = Flag {
      long: "--items".into(),
      short: None,
      help: None,
      arg: None,
      required: false,
      default: None,
      greedy: true,
    };
    let err = flag.validate().unwrap_err().to_string();
    assert!(
      err.contains("must have an 'arg' field"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn flag_greedy_with_arg_ok() {
    let flag = Flag {
      long: "--items".into(),
      short: None,
      help: None,
      arg: Some("ITEM".into()),
      required: false,
      default: None,
      greedy: true,
    };
    assert!(flag.validate().is_ok());
  }

  #[test]
  fn command_empty_name_fails() {
    let cmd = Command {
      name: String::new(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: "echo hi".into(),
      commands: vec![],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(err.contains("cannot be empty"), "unexpected error: {err}");
  }

  #[test]
  fn command_both_command_and_commands_fails() {
    let cmd = Command {
      name: "mixed".into(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: "echo hi".into(),
      commands: vec![leaf_cmd("sub", "echo sub")],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(err.contains("cannot have both"), "unexpected error: {err}");
  }

  #[test]
  fn command_neither_command_nor_commands_fails() {
    let cmd = Command {
      name: "empty".into(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: String::new(),
      commands: vec![],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(err.contains("must have either"), "unexpected error: {err}");
  }

  #[test]
  fn group_command_with_args_fails() {
    let cmd = Command {
      name: "group".into(),
      help: String::new(),
      args: vec![Arg {
        name: "a".into(),
        help: String::new(),
        default: None,
        greedy: false,
      }],
      flags: vec![],
      command: String::new(),
      commands: vec![leaf_cmd("sub", "echo sub")],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(
      err.contains("group commands cannot have args or flags"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn group_command_with_flags_fails() {
    let cmd = Command {
      name: "group".into(),
      help: String::new(),
      args: vec![],
      flags: vec![Flag {
        long: "--verbose".into(),
        short: None,
        help: None,
        arg: None,
        required: false,
        default: None,
        greedy: false,
      }],
      command: String::new(),
      commands: vec![leaf_cmd("sub", "echo sub")],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(
      err.contains("group commands cannot have args or flags"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn valid_leaf_command() {
    let cmd = leaf_cmd("run", "echo run");
    assert!(cmd.validate().is_ok());
  }

  #[test]
  fn valid_group_command() {
    let cmd = Command {
      name: "group".into(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: String::new(),
      commands: vec![leaf_cmd("sub", "echo sub")],
    };
    assert!(cmd.validate().is_ok());
  }

  #[test]
  fn more_than_one_greedy_arg_fails() {
    let cmd = Command {
      name: "multi".into(),
      help: String::new(),
      args: vec![
        Arg {
          name: "a".into(),
          help: String::new(),
          default: None,
          greedy: true,
        },
        Arg {
          name: "b".into(),
          help: String::new(),
          default: None,
          greedy: true,
        },
      ],
      flags: vec![],
      command: "echo hi".into(),
      commands: vec![],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(
      err.contains("only one arg or flag can be greedy"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn greedy_arg_not_last_fails() {
    let cmd = Command {
      name: "order".into(),
      help: String::new(),
      args: vec![
        Arg {
          name: "first".into(),
          help: String::new(),
          default: None,
          greedy: true,
        },
        Arg {
          name: "second".into(),
          help: String::new(),
          default: None,
          greedy: false,
        },
      ],
      flags: vec![],
      command: "echo hi".into(),
      commands: vec![],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(
      err.contains("greedy arg must be the last arg"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn greedy_flag_not_last_fails() {
    let cmd = Command {
      name: "order".into(),
      help: String::new(),
      args: vec![],
      flags: vec![
        Flag {
          long: "--first".into(),
          short: None,
          help: None,
          arg: Some("X".into()),
          required: false,
          default: None,
          greedy: true,
        },
        Flag {
          long: "--second".into(),
          short: None,
          help: None,
          arg: None,
          required: false,
          default: None,
          greedy: false,
        },
      ],
      command: "echo hi".into(),
      commands: vec![],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(
      err.contains("greedy flag must be the last flag"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn one_greedy_arg_last_ok() {
    let cmd = Command {
      name: "ok".into(),
      help: String::new(),
      args: vec![
        Arg {
          name: "first".into(),
          help: String::new(),
          default: None,
          greedy: false,
        },
        Arg {
          name: "rest".into(),
          help: String::new(),
          default: None,
          greedy: true,
        },
      ],
      flags: vec![],
      command: "echo hi".into(),
      commands: vec![],
    };
    assert!(cmd.validate().is_ok());
  }

  #[test]
  fn one_greedy_flag_last_ok() {
    let cmd = Command {
      name: "ok".into(),
      help: String::new(),
      args: vec![],
      flags: vec![
        Flag {
          long: "--normal".into(),
          short: None,
          help: None,
          arg: None,
          required: false,
          default: None,
          greedy: false,
        },
        Flag {
          long: "--rest".into(),
          short: None,
          help: None,
          arg: Some("X".into()),
          required: false,
          default: None,
          greedy: true,
        },
      ],
      command: "echo hi".into(),
      commands: vec![],
    };
    assert!(cmd.validate().is_ok());
  }

  #[test]
  fn recursive_group_validates_subcommands() {
    let cmd = Command {
      name: "parent".into(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: String::new(),
      commands: vec![Command {
        name: String::new(),
        help: String::new(),
        args: vec![],
        flags: vec![],
        command: "echo x".into(),
        commands: vec![],
      }],
    };
    let err = cmd.validate().unwrap_err().to_string();
    assert!(err.contains("cannot be empty"), "unexpected error: {err}");
  }

  #[test]
  fn config_empty_commands_fails() {
    let mut cfg = valid_config();
    cfg.commands.clear();
    let err = cfg.validate().unwrap_err().to_string();
    assert!(
      err.contains("At least one command"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn config_valid_commands_ok() {
    assert!(valid_config().validate().is_ok());
  }

  #[test]
  fn config_validates_nested_command() {
    let mut cfg = valid_config();
    cfg.commands.push(Command {
      name: String::new(),
      help: String::new(),
      args: vec![],
      flags: vec![],
      command: "echo x".into(),
      commands: vec![],
    });
    assert!(cfg.validate().is_err());
  }

  #[test]
  fn load_valid_yaml_config() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.yaml"), valid_config_yaml()).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.device, "/dev/ttyUSB0");
    assert_eq!(config.channel, 1);
    assert!(config.baud.is_none());
    assert_eq!(config.shell, "bash");
    assert_eq!(config.shell_args, vec!["-lc"]);
    assert_eq!(config.max_text_bytes, 200);
    assert_eq!(config.chunk_delay, 10000);
    assert_eq!(config.max_content_bytes, 180);
    assert_eq!(config.commands.len(), 1);
    assert_eq!(config.commands[0].name, "test");
  }

  #[test]
  fn load_config_with_inline_commands() {
    let dir = TempDir::new().unwrap();
    let yaml = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - name: alpha
                command: echo alpha
              - name: beta
                command: echo beta
        "};
    fs::write(dir.path().join("config.yaml"), yaml).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.commands.len(), 2);
    assert_eq!(config.commands[0].name, "alpha");
    assert_eq!(config.commands[1].name, "beta");
  }

  #[test]
  fn load_config_with_import() {
    let dir = TempDir::new().unwrap();

    let imported = indoc! {"
            - name: imported_cmd
              command: echo imported
        "};
    fs::write(dir.path().join("extra.yaml"), imported).unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: extra.yaml
              - name: inline
                command: echo inline
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.commands.len(), 2);
    assert_eq!(config.commands[0].name, "imported_cmd");
    assert_eq!(config.commands[1].name, "inline");
  }

  #[test]
  fn circular_import_detected() {
    let dir = TempDir::new().unwrap();

    let a = indoc! {"
            - import: b.yaml
        "};
    let b = indoc! {"
            - import: a.yaml
        "};
    fs::write(dir.path().join("a.yaml"), a).unwrap();
    fs::write(dir.path().join("b.yaml"), b).unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: a.yaml
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let mut loader = ConfigLoader::new(dir.path());
    let err = loader.load("config.yaml").unwrap_err().to_string();
    assert!(err.contains("Circular import"), "unexpected error: {err}");
  }

  #[test]
  fn missing_import_file_fails() {
    let dir = TempDir::new().unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: nonexistent.yaml
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let err = load_config(dir.path().join("config"))
      .unwrap_err()
      .to_string();
    assert!(
      err.contains("Failed to read file"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn invalid_yaml_fails() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.yaml"), "{{{{not yaml!!!!").unwrap();

    let mut loader = ConfigLoader::new(dir.path());
    let err = loader.load("config.yaml").unwrap_err().to_string();
    assert!(
      err.contains("Failed to parse YAML"),
      "unexpected error: {err}"
    );
  }

  #[test]
  fn load_config_falls_back_to_yml() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.yml"), valid_config_yaml()).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.device, "/dev/ttyUSB0");
    assert_eq!(config.commands.len(), 1);
  }

  #[test]
  fn import_single_command_object() {
    let dir = TempDir::new().unwrap();

    let single = indoc! {"
            name: solo
            command: echo solo
        "};
    fs::write(dir.path().join("solo.yaml"), single).unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: solo.yaml
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.commands.len(), 1);
    assert_eq!(config.commands[0].name, "solo");
  }

  #[test]
  fn nested_import_in_group_command() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("subcommands")).unwrap();

    let subcommand = indoc! {"
            name: inner
            command: echo inner
        "};
    fs::write(dir.path().join("subcommands/inner.yaml"), subcommand).unwrap();

    let group = indoc! {"
            name: outer
            help: Outer group
            commands:
              - import: subcommands/inner.yaml
              - name: direct
                command: echo direct
        "};
    fs::write(dir.path().join("group.yaml"), group).unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: group.yaml
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.commands.len(), 1);
    assert_eq!(config.commands[0].name, "outer");
    assert_eq!(config.commands[0].commands.len(), 2);
    assert_eq!(config.commands[0].commands[0].name, "inner");
    assert_eq!(config.commands[0].commands[1].name, "direct");
  }

  #[test]
  fn deeply_nested_imports() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("level1/level2")).unwrap();

    let leaf = indoc! {"
            name: leaf
            command: echo leaf
        "};
    fs::write(dir.path().join("level1/level2/leaf.yaml"), leaf).unwrap();

    let level2 = indoc! {"
            name: level2
            commands:
              - import: level2/leaf.yaml
        "};
    fs::write(dir.path().join("level1/level2_group.yaml"), level2).unwrap();

    let level1 = indoc! {"
            name: level1
            commands:
              - import: level1/level2_group.yaml
        "};
    fs::write(dir.path().join("level1_group.yaml"), level1).unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: level1_group.yaml
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let config = load_config(dir.path().join("config")).unwrap();
    assert_eq!(config.commands[0].name, "level1");
    assert_eq!(config.commands[0].commands[0].name, "level2");
    assert_eq!(config.commands[0].commands[0].commands[0].name, "leaf");
  }

  #[test]
  fn circular_import_in_nested_commands() {
    let dir = TempDir::new().unwrap();

    let group_a = indoc! {"
            name: group_a
            commands:
              - import: group_b.yaml
        "};
    let group_b = indoc! {"
            name: group_b
            commands:
              - import: group_a.yaml
        "};
    fs::write(dir.path().join("group_a.yaml"), group_a).unwrap();
    fs::write(dir.path().join("group_b.yaml"), group_b).unwrap();

    let main = indoc! {"
            device: /dev/ttyUSB0
            channel: 1
            baud: null
            shell: bash
            shell_args: [\"-lc\"]
            max_text_bytes: 200
            chunk_delay: 10000
            max_content_bytes: 180
            commands:
              - import: group_a.yaml
        "};
    fs::write(dir.path().join("config.yaml"), main).unwrap();

    let mut loader = ConfigLoader::new(dir.path());
    let err = loader.load("config.yaml").unwrap_err().to_string();
    assert!(err.contains("Circular import"), "unexpected error: {err}");
  }

  #[test]
  fn display_file_not_found_contains_path() {
    let err = ConfigError::FileNotFound(
      PathBuf::from("/some/missing/file.yaml"),
      std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
    );
    let msg = err.to_string();
    assert!(
      msg.contains("/some/missing/file.yaml"),
      "unexpected display: {msg}"
    );
  }

  #[test]
  fn display_parse_error_contains_path() {
    let yaml_err = serde_yaml::from_str::<RawConfig>("{{bad").unwrap_err();
    let err = ConfigError::ParseError(PathBuf::from("/bad/config.yaml"), yaml_err);
    let msg = err.to_string();
    assert!(
      msg.contains("/bad/config.yaml"),
      "unexpected display: {msg}"
    );
  }

  #[test]
  fn display_circular_import_contains_path() {
    let err = ConfigError::CircularImport(PathBuf::from("/a/b/loop.yaml"));
    let msg = err.to_string();
    assert!(msg.contains("/a/b/loop.yaml"), "unexpected display: {msg}");
  }

  #[test]
  fn display_validation_error_contains_message() {
    let err = ConfigError::ValidationError("something went wrong".into());
    let msg = err.to_string();
    assert!(
      msg.contains("something went wrong"),
      "unexpected display: {msg}"
    );
  }

  #[test]
  fn display_config_not_found_lists_searched_paths() {
    let err = ConfigError::ConfigNotFound(vec![
      PathBuf::from("./config.yaml"),
      PathBuf::from("./config.yml"),
      PathBuf::from("/home/user/.config/meshexec/config.yaml"),
    ]);
    let msg = err.to_string();
    assert!(
      msg.contains("Config file not found"),
      "unexpected display: {msg}"
    );
    assert!(
      msg.contains("./config.yaml"),
      "should list first searched path: {msg}"
    );
    assert!(
      msg.contains("./config.yml"),
      "should list second searched path: {msg}"
    );
    assert!(
      msg.contains("/home/user/.config/meshexec/config.yaml"),
      "should list third searched path: {msg}"
    );
  }

  #[test]
  fn examples_config_loads_with_recursive_subcommands() {
    let config = load_config("examples/config").unwrap();

    let network = config
      .commands
      .iter()
      .find(|c| c.name == "network")
      .expect("network command not found");
    assert!(
      !network.commands.is_empty(),
      "network should have subcommands"
    );

    let docker = network
      .commands
      .iter()
      .find(|c| c.name == "docker")
      .expect("docker subcommand not found under network");
    assert!(
      !docker.commands.is_empty(),
      "docker should have subcommands"
    );

    let hello = docker
      .commands
      .iter()
      .find(|c| c.name == "hello")
      .expect("hello subcommand not found under docker");
    assert!(
      !hello.command.is_empty(),
      "hello should be a leaf command with a command string"
    );
  }
}
