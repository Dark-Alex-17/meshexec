use std::path::PathBuf;

use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum, command};
use log::LevelFilter;

#[derive(Parser, Debug)]
#[command(
    name = "automesh",
    about = "Execute commands on remote serially-connected nodes based on messages in a private channel"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
    #[command(flatten)]
    pub global: GlobalOpts,
}

#[derive(ClapArgs, Debug)]
#[command(next_help_heading = "Global Options")]
pub struct GlobalOpts {
    /// Specify the config file
    #[arg(long, short, env = "AUTOMESH_CONFIG_FILE")]
    pub config_file: Option<PathBuf>,
    /// Specify the logging level
    #[arg(long, short, value_enum, default_value_t = LogLevel::Info, env = "AUTOMESH_LOG_LEVEL")]
    pub log_level: LogLevel,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the runner server
    Serve,
    /// Tail logs
    TailLogs {
        /// Disable colored log output
        #[arg(long)]
        no_color: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Off => LevelFilter::Off,
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_converts_to_level_filter() {
        assert_eq!(LevelFilter::Off, LevelFilter::from(LogLevel::Off));
        assert_eq!(LevelFilter::Error, LevelFilter::from(LogLevel::Error));
        assert_eq!(LevelFilter::Warn, LevelFilter::from(LogLevel::Warn));
        assert_eq!(LevelFilter::Info, LevelFilter::from(LogLevel::Info));
        assert_eq!(LevelFilter::Debug, LevelFilter::from(LogLevel::Debug));
        assert_eq!(LevelFilter::Trace, LevelFilter::from(LogLevel::Trace));
    }
}
