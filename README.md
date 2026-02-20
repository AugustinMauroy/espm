# ESPM

**ESPM** is a command-line package manager and specification tool written in Rust. It provides functionality for initializing, adding, installing, publishing, and managing packages and dependencies in a transactional and deterministic way. ESPM is designed to work with a custom specification format and to integrate with npm for JavaScript packages.

## Features

- Initialize new packages (`e.g. espm init`)
- Add dependencies (`e.g. espm add <pkg>`)
- Install packages from a spec (`e.g. espm install`)
- Publish packages with dry-run support (`e.g. espm publish`)
- Remove packages (`e.g. espm remove <pkg>`)
- Transactional installs and updates
- Integration with npm via `jsr_npm` module
- Logging and error handling using `anyhow` and structured logs

## Repository Structure

```text
Cargo.toml
src/
    main.rs            # CLI entry point
    cli.rs             # Command parsing and dispatch
    installer.rs       # Package installation logic
    publisher.rs       # Publishing logic
    specifier.rs       # Specification handling
    jsr_npm.rs         # npm-related interaction
    models.rs          # Data models used across the project
    logger.rs          # Logging utilities
tests/                # Integration tests (e2e)
    e2e_*.rs           # End-to-end tests for various commands
```

## Development

1. **Build**: `cargo build`
2. **Test**: `cargo test` (unit and integration tests)
3. **Run**: `cargo run -- <command>`

The project uses typical Rust tooling. Ensure you have a recent Rust toolchain installed.

## Documentation

Documentation is maintained alongside the code in Markdown files such as `spec.md` and `current-state.md`. These documents describe the specification format, current architecture, and design decisions. Refer to them for more in-depth understanding.

## Contributing

Contributions are welcome via pull requests. Please follow the existing code style and include tests for new features or bug fixes.

