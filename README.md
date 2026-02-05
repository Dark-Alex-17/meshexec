# MeshExec - Remote command execution over Meshtastic mesh networks

![License](https://img.shields.io/badge/license-MIT-blueviolet.svg)
[![crates.io link](https://img.shields.io/crates/v/meshexec.svg)](https://crates.io/crates/meshexec)
![Release](https://img.shields.io/github/v/release/Dark-Alex-17/meshexec?color=%23c694ff)
![Crate.io downloads](https://img.shields.io/crates/d/meshexec?label=Crate%20downloads)
[![GitHub Downloads](https://img.shields.io/github/downloads/Dark-Alex-17/meshexec/total.svg?label=GitHub%20downloads)](https://github.com/Dark-Alex-17/meshexec/releases)

MeshExec lets you execute commands on remote serially-connected [Meshtastic](https://meshtastic.org/) nodes by listening for messages in a 
private channel. Define command aliases with arguments and flags in a YAML config, send a message like `!myip` over the 
mesh, and get the output back — no internet required!

## How It Works

1. MeshExec connects to a Meshtastic device via serial port
2. It listens for messages prefixed with `!` on a configured private channel
3. When a matching command alias is received, it executes the corresponding shell command
4. The output is chunked to fit within Meshtastic's message size limits and sent back over the mesh

This makes it ideal for managing remote devices in off-grid, decentralized, or IoT deployments where traditional network 
access isn't available.

## Prerequisites

- A [Meshtastic](https://meshtastic.org/) device connected via serial (USB)
- A private channel configured on the device
- Rust 1.89.0+ (for building from source)

## Installation

### Cargo
If you have Cargo installed, then you can install MeshExec from [Crates.io](https://crates.io/crates/meshexec):

```shell
cargo install meshexec

# If you encounter issues installing, try installing with '--locked'
cargo install --locked meshexec
```

### Homebrew (Mac and Linux)
To install MeshExec from Homebrew, install the MeshExec tap. Then you'll be able to install MeshExec:

```shell
brew tap Dark-Alex-17/meshexec
brew install meshexec

# If you need to be more specific, use the following:
brew install Dark-Alex-17/meshexec/meshexec
```

To upgrade to a newer version of MeshExec:
```shell
brew upgrade meshexec
```

### Manual
Binaries are available on the [releases](https://github.com/Dark-Alex-17/meshexec/releases) page.

#### Linux/macOS Instructions
1. Download the latest [binary](https://github.com/Dark-Alex-17/meshexec/releases) for your OS and architecture.
2. `cd` to the directory where you downloaded the binary.
3. Extract the binary with `tar -C /usr/local/bin -xzf meshexec-<arch>.tar.gz` (Note: This may require `sudo`)
4. Now you can run `meshexec`!

## Usage

MeshExec has two subcommands:

### `meshexec serve`
Starts the runner server that listens for commands on the mesh network:

```shell
# Config file 'config.yml' is in current directory
meshexec serve

# Config file 'config.yml' is in another directory
meshexec --config-file /opt/meshexec/config.yml serve
```

### `meshexec tail-logs`
Tails the MeshExec log file with optional colored output:

```shell
meshexec tail-logs

# Disable colored output
meshexec tail-logs --no-color
```

### Global Options

| Flag                   | Short | Env Var                | Description                                                                       |
|------------------------|-------|------------------------|-----------------------------------------------------------------------------------|
| `--config-file <PATH>` | `-c`  | `MESHEXEC_CONFIG_FILE` | Specify the config file (defaults to `config.yaml` in the current directory)      |
| `--log-level <LEVEL>`  | `-l`  | `MESHEXEC_LOG_LEVEL`   | Set the logging level: `off`, `error`, `warn`, `info` (default), `debug`, `trace` |

### Sending Commands Over the Mesh
Once MeshExec is running, send messages prefixed with `!` on the configured private channel from any node on the mesh:

```
!help                    # List all available commands
!myip                    # Run the 'myip' command
!network check-port 8080 # Run a subcommand with an argument
!loki --help             # Show help for a specific command
```

## Configuration

MeshExec is configured via a YAML file. By default, it looks for `config.yaml` (or `config.yml`) in the current directory. 
You can specify a different path with `--config-file`.

### Example Configuration

```yaml
device: /dev/ttyUSB0
channel: 1
baud: null
shell: bash
shell_args:
  - -lc
max_text_bytes: 200
chunk_delay: 10000
max_content_bytes: 180
commands:
  - import: network_commands.yml

  - name: loki
    help: Ask Loki something
    args:
      - name: question
        help: Your prompt for Loki
        greedy: true
    command: loki "${question}"

  - name: list-disk-space
    help: List disk space for all mounted filesystems
    args:
      - name: servarr
        help: The servarr to hit
    flags:
      - long: --servarr-name
        short: -s
        arg: servarr_name
        help: The name of the servarr instance
    command: |
      # Can define scripts inline
      declare -a flags=()
      if [[ -n $servarr_name ]]; then
        flags+=("--servarr-name $servarr_name")
      fi
      managarr $servarr "${flags[@]}"
```

See the [examples/](examples/) directory for a full configuration example (i.e. with subcommands).

### Configuration Reference

#### Top-Level Fields

| Field               | Type           | Required | Description                                                                                                            |
|---------------------|----------------|----------|------------------------------------------------------------------------------------------------------------------------|
| `device`            | `string`       | Yes      | Serial device path (e.g. `/dev/ttyUSB0`, `/dev/tty.usbserial-0001`)                                                    |
| `channel`           | `integer`      | Yes      | Meshtastic channel number to listen on (must be a **private** channel)                                                 |
| `baud`              | `integer`      | No       | Baud rate for the serial connection (uses the Meshtastic default if `null`)                                            |
| `shell`             | `string`       | Yes      | Shell to execute commands with (e.g. `bash`, `sh`, `zsh`)                                                              |
| `shell_args`        | `list[string]` | No       | Arguments to pass to the shell (e.g. `["-lc"]` for a login shell with command)                                         |
| `max_text_bytes`    | `integer`      | Yes      | Maximum bytes per Meshtastic text message (device-dependent, typically ~200)                                           |
| `chunk_delay`       | `integer`      | Yes      | Delay in milliseconds between sending chunks (prevents flooding the mesh)                                              |
| `max_content_bytes` | `integer`      | Yes      | Maximum content bytes per chunk before footer (should be less than `max_text_bytes` to leave room for `[1/N]` footers) |
| `commands`          | `list`         | Yes      | List of command definitions and/or imports                                                                             |

#### Commands

Commands can be either **leaf commands** (execute a shell command) or **group commands** (contain subcommands). They can 
also be imported from external YAML files, enabling more complex configuration structures.

##### Leaf Command

```yaml
- name: myip
  help: Show the current system's public IP address
  command: curl -s checkip.amazonaws.com
```

| Field     | Type         | Required       | Description                                                                |
|-----------|--------------|----------------|----------------------------------------------------------------------------|
| `name`    | `string`     | Yes            | The alias name (used after `!` prefix, e.g. `!myip`)                       |
| `help`    | `string`     | No             | Help text shown when the user sends `!<command> --help`                    |
| `command` | `string`     | Yes (for leaf) | Shell command to execute. Use `${var_name}` to interpolate arg/flag values |
| `args`    | `list[Arg]`  | No             | Positional arguments                                                       |
| `flags`   | `list[Flag]` | No             | Named flags                                                                |

##### Group Command

Group commands organize subcommands under a namespace:

```yaml
# network_commands.yml
- name: network
  help: Network commands
  commands:
    - name: myip
      command: curl -s checkip.amazonaws.com
    - name: check-port
      args:
        - name: port
          help: The port number to check
      command: 'sudo lsof -i :${port}'
```

| Field      | Type            | Required        | Description             |
|------------|-----------------|-----------------|-------------------------|
| `name`     | `string`        | Yes             | The group name          |
| `help`     | `string`        | No              | Help text for the group |
| `commands` | `list[Command]` | Yes (for group) | Nested subcommands      |

A command **cannot** have both `command` and `commands` — it must be one or the other. Group commands **cannot** have 
`args` or `flags`.

##### Importing Commands

Commands can be split across multiple YAML files using imports:

```yaml
commands:
  - import: network_commands.yml
  - import: monitoring_commands.yml
  - name: inline-command
    command: echo "I'm defined inline"
```

The imported file can contain either a single command object or a list of commands. Circular imports are detected and
will produce an error.

#### Args (Positional Arguments)

| Field     | Type     | Required | Description                                                                       |
|-----------|----------|----------|-----------------------------------------------------------------------------------|
| `name`    | `string` | Yes      | Argument name (used as the environment variable name; hyphens become underscores) |
| `help`    | `string` | Yes      | Help text shown in `--help` output                                                |
| `default` | `string` | No       | Default value if not provided (if omitted, the argument is required)              |
| `greedy`  | `bool`   | No       | If `true`, consumes all remaining tokens. Must be the last arg. Default: `false`  |

#### Flags

| Field      | Type     | Required | Description                                                                                                              |
|------------|----------|----------|--------------------------------------------------------------------------------------------------------------------------|
| `long`     | `string` | Yes      | Long flag name (must start with `--`, e.g. `--verbose`)                                                                  |
| `short`    | `string` | No       | Short flag alias (must be `-` followed by a single character, e.g. `-v`)                                                 |
| `help`     | `string` | No       | Help text shown in `--help` output                                                                                       |
| `arg`      | `string` | No       | If present, the flag takes a value (the string is the env var name). If absent, the flag is boolean                      |
| `required` | `bool`   | No       | If `true`, the flag must be provided. Default: `false`                                                                   |
| `default`  | `string` | No       | Default value when the flag is not provided                                                                              |
| `greedy`   | `bool`   | No       | If `true`, consumes all remaining tokens as the value. Requires `arg` to be set. Must be the last flag. Default: `false` |

#### Greedy Behavior

Only **one** arg or flag in a command can be greedy, and it must be the **last** in its respective list. A greedy 
arg/flag consumes all remaining whitespace-separated tokens as a single value. This is useful for free-text inputs:

```yaml
- name: ask
  help: Ask a question
  args:
    - name: question
      help: Your question
      greedy: true
  command: echo "${question}"
```

Sending `!ask what is the weather today` would set `question` to `"what is the weather today"`.

## Environment Variables

| Variable               | Description                                                      | Equivalent Flag |
|------------------------|------------------------------------------------------------------|-----------------|
| `MESHEXEC_CONFIG_FILE` | Path to the config file                                          | `--config-file` |
| `MESHEXEC_LOG_LEVEL`   | Logging level (`off`, `error`, `warn`, `info`, `debug`, `trace`) | `--log-level`   |

## Contributing
See the [CONTRIBUTING.md](CONTRIBUTING.md) for details on how to contribute to this project.

## Dependencies
* [meshtastic](https://github.com/meshtastic/rust) - Meshtastic protocol library for Rust
* [clap](https://github.com/clap-rs/clap) - Command line argument parsing
* [tokio](https://github.com/tokio-rs/tokio) - Async runtime
* [serde](https://github.com/serde-rs/serde) - Serialization/deserialization framework
* [log4rs](https://github.com/estk/log4rs) - Logging framework

## Creator
* [Alex Clarke](https://github.com/Dark-Alex-17)
