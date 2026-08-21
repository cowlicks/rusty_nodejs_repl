# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->

## [Unreleased] - ReleaseDate

### Added

### Changed

### Removed



## [0.5.0] - 2026-08-21

### Added

- `Repl::kill` and `Repl::wait_or_kill` for ending the Node.js process directly.
- `DEFAULT_STOP_TIMEOUT`, the grace period `Repl::stop` gives Node.js to exit on its own.

### Changed

- The Node.js process is no longer leaked. Previously it kept running whenever the `Repl` was
  dropped without `stop()` (a panicking test, an early `?`), and even `stop()` left it running
  forever if the JS event loop still had open handles. Now: the child is spawned with
  `kill_on_drop`, `stop()` waits `DEFAULT_STOP_TIMEOUT` and then kills, the REPL script exits
  itself when stdin closes or it gets a `SIGHUP`/`SIGINT`/`SIGTERM`, and the default command
  `exec`s so the process we hold is Node.js rather than the shell.

### Removed



## [0.4.0] - 2026-02-10

### Added

* Added `run_make` to `integration_utils`

### Changed

### Removed



## [0.3.0] - 2026-01-30

This is a breaking change due `Repl.run` changing. To output data you must now pass it through the JavaScript `output` function, instead of via stdout. This passes the data through a TCP socket, and prevents spurious `console.logs` from effecting the data you are trying to read.

### Added

* New unit tests: error handling for syntax errors, `Config::imports`, `Config::before`, `str_run`, empty output, binary data through socket, `fmt_rs_vec_u8_as_js_buf` helper, and custom EOF delimiter
* `outputJson` is now in the JavaScript scope. It's just `(x) => s.write(JSON.stringify(x))`

### Changed

* `Repl::run` and `Repl::json_run` now uses TCP socket by default
* Fixed bug where `Config` imports/before/after with single items were missing trailing semicolons

### Removed



## [0.2.2] - 2025-12-09

### Added

* Add `Repl::run_tcp` which reads data from a socket which connects the JavaScript and Rust process (instead of stdout).
* Add `Repl::json_run` which runs json and parses stdout into a Rust type. Behind the `serde` feature flag.
* Add integration utils (behind a `integration_utils` flag) for testing. Includes a `git_root` function and `join_paths` macro, and a `log` function for instantiating logs.

### Changed

### Removed



## [0.2.1] - 2024-08-27

### Added

### Changed

* Fixed a bug in `Repl::stop`

### Removed



## [0.2.0] - 2024-08-25

### Added

### Changed

More docs. Change Error variants

### Removed



## [0.1.2] - 2024-08-24

### Added

### Changed

Rename `Repl::repl` to `Repl::run`.

### Removed



## [0.1.1] - 2024-08-22

### Added

- Added some metadata for docs.rs

### Changed


### Removed

<!-- next-url -->
[Unreleased]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/cowlicks/rusty_nodejs_repl/compare/v0.1.0...v0.1.1
