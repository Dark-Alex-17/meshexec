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
    Command(Command),
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
                CommandEntry::Command(cmd) => {
                    resolved.push(cmd.clone());
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

        let content = fs::read_to_string(path)
            .map_err(|e| ConfigError::FileNotFound(path.to_path_buf(), e))?;

        if let Ok(cmd) = serde_yaml::from_str::<Command>(&content) {
            return Ok(vec![cmd]);
        }

        let entries: Vec<CommandEntry> = serde_yaml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(path.to_path_buf(), e))?;

        self.resolve_commands(&entries, path)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    FileNotFound(PathBuf, std::io::Error),
    ParseError(PathBuf, serde_yaml::Error),
    CircularImport(PathBuf),
    ValidationError(String),
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
