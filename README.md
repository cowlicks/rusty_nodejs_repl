# Rusty Node.js REPL

## What?

A way run arbitrary code through Node.js from Rust.

## Why?

This is for **testing**. When working on Node.js related Rust projects it can allow you to co-locate JavaScript along side your Rust.


## Usage

Put some JavaScript in a string and pass it to `JsContext::repl`. The function will return whatever was sent to `stdout`:

```rust
let mut context = ReplConf::build()?.start()?;
let result = context.repl("console.log('Hello, world!');").await?;
assert_eq!(result, b"Hello, world!\n");
```
