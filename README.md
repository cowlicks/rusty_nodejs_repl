# Rusty Node.js REPL

## What?

A way run arbitrary code through Node.js from Rust.

## Why?

This is for **testing**. When working on Node.js related Rust projects it can allow you to co-locate JavaScript along side your Rust.

This crate came from implementing parts of the [Hypercore JS ecosystem](https://docs.pears.com/building-blocks/hypercore) where there is a need to test a Rust implementation against the JavaScript implementation.


## Usage

Put some JavaScript in a string and pass it to `JsContext::repl`. The function will return whatever was sent to `stdout`:

```rust
let mut context = Config::build()?.start()?;
let result = context.repl("console.log('Hello, world!');").await?;
assert_eq!(result, b"Hello, world!\n");
```


For more in-depth usage see the test in the [Rust Hypercore Replicator](https://github.com/cowlicks/replicator/tree/master/replicator/tests).
