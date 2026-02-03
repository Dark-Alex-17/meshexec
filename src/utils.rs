use crate::cli::LogLevel;
use crate::config::Config;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use log::{LevelFilter, error, info};
use log4rs::append::console::ConsoleAppender;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use meshtastic::api::ConnectedStreamApi;
use meshtastic::api::state::Configured;
use meshtastic::packet::{PacketDestination, PacketReceiver, PacketRouter};
use meshtastic::protobufs::from_radio;
use meshtastic::types::MeshChannel;
use regex::Regex;
use std::error::Error;
use std::fmt::Display;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;
use std::{fs, mem};
use tokio::time::{sleep, timeout};

pub async fn wait_for_my_node_num(rx: &mut PacketReceiver) -> Result<u32> {
    let msg = timeout(Duration::from_secs(10), async {
        loop {
            let fr = rx.recv().await?;
            if let Some(from_radio::PayloadVariant::MyInfo(my_info)) = fr.payload_variant {
                return Some(my_info);
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for MyInfo. Is node online and connected?"))?
    .ok_or_else(|| anyhow!("rx closed before receiving MyInfo"))?;

    let node_num = msg.my_node_num;
    Ok(node_num)
}

pub fn chunk_lines_with_footer(text: &str, max_bytes: usize) -> Vec<String> {
    assert!(max_bytes > 0);

    let mut raw_chunks = Vec::new();
    let mut current = String::new();
    let mut current_bytes = 0usize;

    for line in text.split_inclusive('\n') {
        let line_bytes = line.len();

        if line_bytes > max_bytes {
            if !current.is_empty() {
                raw_chunks.push(mem::take(&mut current));
                current_bytes = 0;
            }

            let mut end = max_bytes.min(line.len());
            while end > 0 && !line.is_char_boundary(end) {
                end -= 1;
            }

            raw_chunks.push(line[..end].to_string());
            continue;
        }

        if current_bytes + line_bytes > max_bytes {
            raw_chunks.push(mem::take(&mut current));
            current_bytes = 0;
        }

        current.push_str(line);
        current_bytes += line_bytes;
    }

    if !current.is_empty() {
        raw_chunks.push(current);
    }

    let total = raw_chunks.len();

    raw_chunks
        .into_iter()
        .enumerate()
        .map(|(i, mut chunk)| {
            if total > 1 {
                let footer = format!("\n\n[{}/{}]", i + 1, total);
                let footer_bytes = footer.len();

                let available = max_bytes.saturating_sub(footer_bytes);
                if chunk.len() > available {
                    let mut end = available.min(chunk.len());
                    while end > 0 && !chunk.is_char_boundary(end) {
                        end -= 1;
                    }
                    chunk.truncate(end);
                }

                chunk.push_str(&footer);
            }
            chunk
        })
        .collect()
}

pub async fn send_split_text<R, E>(
    api: &mut ConnectedStreamApi<Configured>,
    router: &mut R,
    text: &str,
    server_config: &Config,
) -> Result<()>
where
    E: Display + Error + Send + Sync + 'static,
    R: PacketRouter<(), E>,
{
    let chunks = chunk_lines_with_footer(text, server_config.max_content_bytes);

    for (idx, part) in chunks.iter().enumerate() {
        info!("Sending chunk: {part}");
        let bytes = part.len();
        if bytes > server_config.max_text_bytes {
            error!(
                "part {} is {bytes} bytes (> {})",
                idx + 1,
                server_config.max_text_bytes
            );
            continue;
        }

        match api
            .send_text(
                router,
                part.clone(),
                PacketDestination::Broadcast,
                false,
                MeshChannel::from(server_config.channel),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("send_text failed on part {}: {e}", idx + 1);
                sleep(Duration::from_millis(server_config.chunk_delay)).await;
                api.send_text(
                    router,
                    part.clone(),
                    PacketDestination::Broadcast,
                    false,
                    MeshChannel::from(server_config.channel),
                )
                .await?;
            }
        }

        sleep(Duration::from_millis(server_config.chunk_delay)).await;
    }

    Ok(())
}

pub fn get_log_path() -> PathBuf {
    let mut log_path = if cfg!(target_os = "linux") {
        dirs_next::cache_dir().unwrap_or_else(|| PathBuf::from("~/.cache"))
    } else if cfg!(target_os = "macos") {
        dirs_next::home_dir().unwrap().join("Library/Logs")
    } else {
        dirs_next::data_local_dir().unwrap_or_else(|| PathBuf::from("C:\\Logs"))
    };

    log_path.push("automesh");

    if let Err(e) = fs::create_dir_all(&log_path) {
        eprintln!("Failed to create log directory: {e:?}");
    }

    log_path.push("automesh.log");
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
