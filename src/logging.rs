use crate::cli::LogLevel;
use anyhow::{Context, Result};
use colored::Colorize;
use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use regex::Regex;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;

pub fn get_log_path() -> PathBuf {
  let mut log_path = if cfg!(target_os = "linux") {
    dirs_next::cache_dir().unwrap_or_else(|| PathBuf::from("~/.cache"))
  } else if cfg!(target_os = "macos") {
    dirs_next::home_dir().unwrap().join("Library/Logs")
  } else {
    dirs_next::data_local_dir().unwrap_or_else(|| PathBuf::from("C:\\Logs"))
  };

  log_path.push("meshexec");

  if let Err(e) = fs::create_dir_all(&log_path) {
    eprintln!("Failed to create log directory: {e:?}");
  }

  log_path.push("meshexec.log");
  log_path
}

pub fn init_logging_config(log_level: LogLevel) -> log4rs::Config {
  let encoder = Box::new(PatternEncoder::new(
    "{d(%Y-%m-%d %H:%M:%S%.3f)(utc)} <{i}> [{l}] {f}:{L} - {m}{n}",
  ));
  let logfile = FileAppender::builder()
    .encoder(encoder.clone())
    .build(get_log_path())
    .unwrap();
  let stdout = ConsoleAppender::builder().encoder(encoder.clone()).build();

  log4rs::Config::builder()
    .appender(Appender::builder().build("logfile", Box::new(logfile)))
    .appender(Appender::builder().build("stdout", Box::new(stdout)))
    .logger(Logger::builder().build("meshtastic::connections::stream_buffer", LevelFilter::Off))
    .build(
      Root::builder()
        .appender("logfile")
        .appender("stdout")
        .build(log_level.into()),
    )
    .unwrap()
}

pub async fn tail_logs(no_color: bool) -> Result<()> {
  let re = Regex::new(
    r"^(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s+<(?P<opid>[^\s>]+)>\s+\[(?P<level>[A-Z]+)]\s+(?P<logger>[^:]+):(?P<line>\d+)\s+-\s+(?P<message>.*)$",
  )?;
  let file_path = get_log_path();
  let file = File::open(&file_path).expect("Cannot open file");
  let mut reader = BufReader::new(file);

  reader
    .seek(SeekFrom::End(0))
    .with_context(|| "Unable to tail log file")?;

  let mut lines = reader.lines();

  tokio::spawn(async move {
    loop {
      if let Some(Ok(line)) = lines.next() {
        if no_color {
          println!("{line}");
        } else {
          let colored_line = colorize_log_line(&line, &re);
          println!("{colored_line}");
        }
      }
    }
  })
  .await?
}

fn colorize_log_line(line: &str, re: &Regex) -> String {
  if let Some(caps) = re.captures(line) {
    let level = &caps["level"];
    let message = &caps["message"];

    let colored_message = match level {
      "ERROR" => message.red(),
      "WARN" => message.yellow(),
      "INFO" => message.green(),
      "DEBUG" => message.blue(),
      _ => message.normal(),
    };

    let timestamp = &caps["timestamp"];
    let opid = &caps["opid"];
    let logger = &caps["logger"];
    let line_number = &caps["line"];

    format!(
      "{} <{}> [{}] {}:{} - {}",
      timestamp.white(),
      opid.cyan(),
      level.bold(),
      logger.magenta(),
      line_number.bold(),
      colored_message
    )
  } else {
    line.to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn log_regex() -> Regex {
    Regex::new(
            r"^(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s+<(?P<opid>[^\s>]+)>\s+\[(?P<level>[A-Z]+)]\s+(?P<logger>[^:]+):(?P<line>\d+)\s+-\s+(?P<message>.*)$",
        )
        .unwrap()
  }

  #[test]
  fn colorize_error_log_line_returns_colored_message() {
    let line = "2025-01-15 12:00:00.000 <main> [ERROR] src/main.rs:42 - Boom";
    let colored = colorize_log_line(line, &log_regex());
    assert!(!colored.is_empty());
    assert!(colored.contains("Boom"));
  }

  #[test]
  fn colorize_info_log_line_returns_colored_message() {
    let line = "2025-01-15 12:00:00.000 <main> [INFO] src/main.rs:42 - Hello world";
    let colored = colorize_log_line(line, &log_regex());
    assert!(!colored.is_empty());
    assert!(colored.contains("Hello world"));
  }

  #[test]
  fn colorize_non_matching_line_returns_original() {
    let line = "not a log line";
    let colored = colorize_log_line(line, &log_regex());
    assert_eq!(colored, line);
  }

  #[test]
  fn colorize_empty_string_returns_empty() {
    let colored = colorize_log_line("", &log_regex());
    assert_eq!(colored, "");
  }

  #[test]
  fn get_log_path_has_expected_suffix_and_is_absolute() {
    let path = get_log_path();
    let components: Vec<_> = path.components().collect();
    let len = components.len();
    assert!(len >= 2);
    assert_eq!(
      components[len - 2].as_os_str(),
      "meshexec",
      "Expected parent directory to be 'meshexec'"
    );
    assert_eq!(
      components[len - 1].as_os_str(),
      "meshexec.log",
      "Expected file name to be 'meshexec.log'"
    );
    assert!(path.is_absolute());
  }
}
