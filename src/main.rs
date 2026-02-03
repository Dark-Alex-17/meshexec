use anyhow::{Context, Result};
use automesh::cli::{Args, Commands};
use automesh::command::{self, AliasResult};
use automesh::config::{Config, load_config};
use automesh::logging::{init_logging_config, tail_logs};
use automesh::transport::{send_split_text, wait_for_my_node_num};
use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
use indoc::formatdoc;
use log::{debug, error, info, warn};
use meshtastic::packet::PacketRouter;
use meshtastic::protobufs::{FromRadio, MeshPacket};
use meshtastic::types::NodeId;
use meshtastic::utils::generate_rand_id;
use meshtastic::{
    api::StreamApi,
    protobufs::{PortNum, from_radio, mesh_packet},
    utils::stream::build_serial_stream,
};
use std::collections::HashMap;
use std::convert::Infallible;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::process::Command;
use std::str::from_utf8;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{env, io, panic, process};
use tokio::signal;
use tokio_util::sync::CancellationToken;

static DEFAULT_CONFIG_FILENAME: &str = "config.yaml";

#[tokio::main]
async fn main() -> Result<()> {
    panic::set_hook(Box::new(|info| {
        panic_hook(info);
    }));
    let args = Args::parse();
    log4rs::init_config(init_logging_config(args.global.log_level))?;
    let default_config_file = PathBuf::from(DEFAULT_CONFIG_FILENAME);
    let config = load_config(
        args.global
            .config_file
            .as_deref()
            .unwrap_or(&default_config_file),
    )?;
    debug!("Loaded config: {config:?}");
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    let cancellation_token = CancellationToken::new();
    let ctrlc_cancellation_token = cancellation_token.clone();
    ctrlc::set_handler(move || {
        ctrlc_cancellation_token.cancel();
        r.store(false, Ordering::SeqCst);
        process::exit(1);
    })
    .expect("Error setting Ctrl-C handler");

    match args.command {
        Commands::TailLogs { no_color } => tail_logs(no_color).await?,
        Commands::Serve => start_runner_server(config).await?,
    }

    Ok(())
}

async fn start_runner_server(server_config: Config) -> Result<()> {
    let serial = build_serial_stream(server_config.device.clone(), server_config.baud, None, None)?;

    let (mut rx, api) = StreamApi::new().connect(serial).await;
    let config_id = generate_rand_id();
    let mut api = api.configure(config_id).await?;
    let node_id = wait_for_my_node_num(&mut rx).await?;
    let mut router = NoopRouter::new(NodeId::new(node_id));

    info!("Connected to {}", server_config.device);
    warn!(
        "\n{}",
        formatdoc! {"
        *****************************************
        CAUTION: Be sure channel {} is private!
        *****************************************
        ",
            server_config.channel
        }
    );
    info!(
        "Listening for commands in channel {}... \n(Ctrl+C to stop)",
        server_config.channel
    );

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                error!("Ctrl+C received, exiting.");
                break;
            }

            maybe = rx.recv() => {
                let Some(from_radio_msg) = maybe else { break; };

                let Some(from_radio::PayloadVariant::Packet(packet)) = from_radio_msg.payload_variant else {
                    continue;
                };

                if packet.channel != server_config.channel {
                    continue;
                }

                let Some(mesh_packet::PayloadVariant::Decoded(data)) = packet.payload_variant else {
                    continue;
                };

                if packet.from == node_id {
                    continue;
                }

                let portnum = PortNum::try_from(data.portnum).ok();

                let message = match from_utf8(&data.payload) {
                    Ok(s) => s.trim_end(),
                    Err(_) => {
                        error!(
                            "[ch {}] {:?}: <{} bytes>",
                            packet.channel,
                            portnum.unwrap_or(PortNum::UnknownApp),
                            data.payload.len()
                        );
                        continue;
                    }
                };

                if !message.starts_with('!') {
                    debug!("Ignoring non-alias message.");
                    continue;
                }

                let (resolved, alias_env) = match command::resolve_alias(message, &server_config.commands) {
                    Ok(AliasResult::HelpText(text)) => {
                        send_split_text(&mut api, &mut router, &text, &server_config).await?;
                        continue;
                    }
                    Ok(AliasResult::Command { command, env }) => (command, env),
                    Err(e) => {
                        warn!("Alias error: {e}");
                        send_split_text(&mut api, &mut router, &e.to_string(), &server_config).await?;
                        continue;
                    }
                };

                info!("Executing: {resolved}");
                let path = env::var("PATH").context("No PATH environment variable")?;
                let mut envs: HashMap<String, String> = HashMap::new();
                envs.insert("PATH".into(), path);
                envs.extend(alias_env);
                let output = Command::new(&server_config.shell)
                    .args(&server_config.shell_args)
                    .arg(&resolved)
                    .envs(envs)
                    .output();
                match output {
                    Ok(out) => {
                        let status = out.status;
                        let stdout = from_utf8(&out.stdout).context("Invalid UTF-8 in stdout")?;
                        let stderr = from_utf8(&out.stderr).context("Invalid UTF-8 in stderr")?;

                        if !status.success() {
                            let err = if !stderr.is_empty() {
                                stderr.to_owned()
                            } else {
                                "Command exited with non-zero status.".into()
                            };
                            send_split_text(&mut api, &mut router, &err, &server_config).await?;
                        }
                        send_split_text(&mut api, &mut router, stdout, &server_config).await?;
                    }
                    Err(e) => {
                        send_split_text(&mut api, &mut router, &format!("Error: {e:?}"), &server_config).await?;
                    }
                }
            }
        }
    }

    Ok(())
}

pub struct NoopRouter {
    source: NodeId,
}

impl NoopRouter {
    pub fn new(source: NodeId) -> Self {
        Self { source }
    }
}

impl PacketRouter<(), Infallible> for NoopRouter {
    fn handle_packet_from_radio(&mut self, _packet: FromRadio) -> Result<(), Infallible> {
        Ok(())
    }

    fn handle_mesh_packet(&mut self, _packet: MeshPacket) -> Result<(), Infallible> {
        Ok(())
    }

    fn source_node_id(&self) -> NodeId {
        self.source
    }
}

#[cfg(debug_assertions)]
fn panic_hook(info: &PanicHookInfo<'_>) {
    use backtrace::Backtrace;
    use crossterm::style::Print;

    let location = info.location().unwrap();

    let msg = match info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    };

    let stacktrace: String = format!("{:?}", Backtrace::new()).replace('\n', "\n\r");

    disable_raw_mode().unwrap();
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        Print(format!(
            "thread '<unnamed>' panicked at '{msg}', {location}\n\r{stacktrace}"
        )),
    )
    .unwrap();
}

#[cfg(not(debug_assertions))]
fn panic_hook(info: &PanicHookInfo<'_>) {
    use human_panic::{handle_dump, metadata, print_msg};

    let meta = metadata!();
    let file_path = handle_dump(&meta, info);
    disable_raw_mode().unwrap();
    execute!(io::stdout(), LeaveAlternateScreen).unwrap();
    print_msg(file_path, &meta).expect("human-panic: printing error message to console failed");
}
