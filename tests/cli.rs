use std::time::Duration;

use assert_cmd::{Command, cargo::cargo_bin_cmd};
use predicates::prelude::*;

fn automesh() -> Command {
    let mut cmd = cargo_bin_cmd!("automesh");
    cmd.timeout(Duration::from_secs(10));
    cmd
}

#[test]
fn no_subcommand_exits_with_error() {
    automesh()
        .assert()
        .failure()
        .stderr(predicates::str::contains("Usage"));
}

#[test]
fn help_flag_shows_usage() {
    automesh().arg("--help").assert().success().stdout(
        predicates::str::contains("automesh")
            .and(predicates::str::contains("Execute commands"))
            .and(predicates::str::contains("serve"))
            .and(predicates::str::contains("tail-logs")),
    );
}

#[test]
fn serve_help_shows_description() {
    automesh()
        .args(["serve", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Start the runner server"));
}

#[test]
fn tail_logs_help_shows_description_and_no_color_flag() {
    automesh()
        .args(["tail-logs", "--help"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("Tail logs").and(predicates::str::contains("--no-color")),
        );
}

#[test]
fn unknown_subcommand_exits_with_error() {
    automesh().arg("foobar").assert().failure();
}

#[test]
fn log_level_accepts_all_valid_values() {
    for level in ["off", "error", "warn", "info", "debug", "trace"] {
        automesh()
            .args(["--log-level", level, "serve", "--help"])
            .assert()
            .success();
    }
}

#[test]
fn log_level_rejects_invalid_value() {
    automesh()
        .args(["--log-level", "banana", "serve"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid value"));
}

#[test]
fn short_log_level_flag_works() {
    automesh()
        .args(["-l", "debug", "serve", "--help"])
        .assert()
        .success();
}

#[test]
fn config_file_flag_accepts_path() {
    automesh()
        .args(["--config-file", "/tmp/fake.yaml", "serve", "--help"])
        .assert()
        .success();
}

#[test]
fn short_config_file_flag_accepts_path() {
    automesh()
        .args(["-c", "/tmp/fake.yaml", "serve", "--help"])
        .assert()
        .success();
}

#[test]
fn env_var_automesh_log_level_is_accepted() {
    automesh()
        .env("AUTOMESH_LOG_LEVEL", "debug")
        .args(["serve", "--help"])
        .assert()
        .success();
}

#[test]
fn env_var_automesh_config_file_is_accepted() {
    automesh()
        .env("AUTOMESH_CONFIG_FILE", "/tmp/fake.yaml")
        .args(["serve", "--help"])
        .assert()
        .success();
}

#[test]
fn serve_fails_at_runtime_not_arg_parsing() {
    automesh()
        .arg("serve")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Usage").not());
}

#[test]
fn tail_logs_fails_at_runtime_not_arg_parsing() {
    automesh()
        .arg("tail-logs")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Usage").not());
}
