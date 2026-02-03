# Contributing
Contributors are very welcome! **No contribution is too small and all contributions are valued.**

## License

Automesh is licensed under the [MIT License](LICENSE). You are free to use, modify, and distribute this software as permitted by the license.

## Rust

You'll need to have the stable Rust toolchain installed in order to develop Automesh. The minimum supported Rust version is **1.89.0**.

The Rust toolchain (stable) can be installed via rustup using the following command:

```shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

This will install `rustup`, `rustc` and `cargo`. For more information, refer to the [official Rust installation documentation](https://www.rust-lang.org/tools/install).

## Setup Workspace

1. Clone this repo
2. Run `cargo test` to verify your setup
3. Make changes
4. Build the application using `just build` or `cargo build`
   - Install `just` (`cargo install just`) if you haven't already to use the [justfile](./justfile) in this project.
5. Run tests with `just test` and linting with `just lint`. If there are errors or warnings from Clippy, please fix them.
6. Format your code with `just fmt`
7. Push your code to a new branch named after the feature/bug/etc. you're adding
8. Create a PR

## Questions? Reach out to me!

If you encounter any questions while developing Automesh, please don't hesitate to reach out to me at alex.j.tusa@gmail.com. I'm happy to help contributors, new and experienced, in any way I can!
