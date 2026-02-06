VERSION := "latest"
IMG_NAME := "darkalex17/meshexec"

# List all recipes
default:
    @just --list

# Run all tests
[group: 'test']
@test:
	cargo test --all

# See what linter errors and warnings are unaddressed
[group: 'style']
@lint:
	cargo clippy --all

# Run Rustfmt against all source files
[group: 'style']
@fmt:
	cargo fmt --all

# Build the project for the current system architecture
# (Gets stored at ./target/[debug|release]/automesh)
[group: 'build']
[arg('build_type', pattern="debug|release")]
@build build_type='debug':
	@cargo build {{ if build_type == "release" { "--release" } else { "" } }}

# Build the docker image
[group: 'build']
build-docker:
    @DOCKER_BUILDKIT=1 docker build --rm -t {{IMG_NAME}}:{{VERSION}} .
