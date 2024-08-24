# Rusty Node.js REPL 🦀 🌳 🌀

```
$ cargo add rusty_nodejs_repl
```

## Usage

Put some JavaScript in a string and pass it to `Repl::run`. The function will return whatever was sent to `stdout`:

```rust
let mut repl = Config::build()?.start()?;
let result = repl.run("console.log('Hello, world!');").await?;
assert_eq!(result, b"Hello, world!\n");
```

[Documentation.](https://docs.rs/rusty_nodejs_repl/latest/rusty_nodejs_repl/)

For more in-depth usage see the test in the [Rust Hypercore Replicator](https://github.com/cowlicks/replicator/blob/af7eda1979d98c40c8b46a1113b5c8b1100b41d5/replicator/tests/js_integration.rs#L62-L87).

## What?

A way run arbitrary code through Node.js from Rust.

## Why?

This is for **testing**. When working on Node.js related Rust projects it can allow you to co-locate JavaScript along side your Rust.

This crate came from implementing parts of the [Hypercore JS ecosystem](https://docs.pears.com/building-blocks/hypercore) where I needed need to test Rust output against JavaScript.
