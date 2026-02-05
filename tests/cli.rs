use std::time::Duration;

use assert_cmd::{Command, cargo::cargo_bin_cmd};
use predicates::prelude::*;

fn meshexec() -> Command {
  let mut cmd = cargo_bin_cmd!("meshexec");
  cmd.timeout(Duration::from_secs(10));
  cmd
}

#[test]
fn no_subcommand_exits_with_error() {
  meshexec()
    .assert()
    .failure()
    .stderr(predicates::str::contains("Usage"));
}

#[test]
fn help_flag_shows_usage() {
  meshexec().arg("--help").assert().success().stdout(
    predicates::str::contains("meshexec")
      .and(predicates::str::contains("aliases"))
      .and(predicates::str::contains("execute"))
      .and(predicates::str::contains("remotely"))
      .and(predicates::str::contains("serve"))
      .and(predicates::str::contains("tail-logs"))
      .and(predicates::str::contains("config-path")),
  );
}

#[test]
fn serve_help_shows_description() {
  meshexec()
    .args(["serve", "--help"])
    .assert()
    .success()
    .stdout(predicates::str::contains("Start the runner server"));
}

#[test]
fn tail_logs_help_shows_description_and_no_color_flag() {
  meshexec()
    .args(["tail-logs", "--help"])
    .assert()
    .success()
    .stdout(predicates::str::contains("Tail logs").and(predicates::str::contains("--no-color")));
}

#[test]
fn unknown_subcommand_exits_with_error() {
  meshexec().arg("foobar").assert().failure();
}

#[test]
fn log_level_accepts_all_valid_values() {
  for level in ["off", "error", "warn", "info", "debug", "trace"] {
    meshexec()
      .args(["--log-level", level, "serve", "--help"])
      .assert()
      .success();
  }
}

#[test]
fn log_level_rejects_invalid_value() {
  meshexec()
    .args(["--log-level", "banana", "serve"])
    .assert()
    .failure()
    .stderr(predicates::str::contains("invalid value"));
}

#[test]
fn short_log_level_flag_works() {
  meshexec()
    .args(["-l", "debug", "serve", "--help"])
    .assert()
    .success();
}

#[test]
fn config_file_flag_accepts_path() {
  meshexec()
    .args(["--config-file", "/tmp/fake.yaml", "serve", "--help"])
    .assert()
    .success();
}

#[test]
fn short_config_file_flag_accepts_path() {
  meshexec()
    .args(["-c", "/tmp/fake.yaml", "serve", "--help"])
    .assert()
    .success();
}

#[test]
fn env_var_meshexec_log_level_is_accepted() {
  meshexec()
    .env("MESHEXEC_LOG_LEVEL", "debug")
    .args(["serve", "--help"])
    .assert()
    .success();
}

#[test]
fn env_var_meshexec_config_file_is_accepted() {
  meshexec()
    .env("MESHEXEC_CONFIG_FILE", "/tmp/fake.yaml")
    .args(["serve", "--help"])
    .assert()
    .success();
}

#[test]
fn serve_fails_at_runtime_not_arg_parsing() {
  meshexec()
    .arg("serve")
    .assert()
    .failure()
    .stderr(predicates::str::contains("Usage").not());
}

#[test]
fn tail_logs_fails_at_runtime_not_arg_parsing() {
  meshexec()
    .arg("tail-logs")
    .assert()
    .failure()
    .stderr(predicates::str::contains("Usage").not());
}

#[test]
fn config_path_help_shows_description() {
  meshexec()
    .args(["config-path", "--help"])
    .assert()
    .success()
    .stdout(predicates::str::contains(
      "Print the default config file path",
    ));
}

#[test]
fn config_path_succeeds_and_prints_path() {
  meshexec()
    .arg("config-path")
    .assert()
    .success()
    .stdout(predicates::str::contains("meshexec").and(predicates::str::contains("config.yaml")));
}
