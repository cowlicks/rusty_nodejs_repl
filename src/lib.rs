/*!
This crate lets you run arbitrary code in a  Node.js REPL from Rust.
Use [`Config`] to setup the REPL and use [`Repl`] to interact with it.
```rust
# tokio_test::block_on(async {
# use rusty_nodejs_repl::{Repl, Config, Error};
let mut repl: Repl = Config::build()?.start()?;
let result = repl.run("console.log('Hello, world!');").await?;
assert_eq!(result, b"Hello, world!\n");
repl.stop().await?;
# Ok::<(),Error>(())
# }).unwrap();
```
The REPL is run in it's own [`tempfile::TempDir`]. So any files created alongside it will be cleaned up on exit.
*/
#![warn(missing_debug_implementations, missing_docs)]
use futures_lite::{io::Bytes, AsyncReadExt, AsyncWriteExt, StreamExt};

use std::{fs::File, io::Write, process::Command, string::FromUtf8Error};

use async_process::{ChildStdout, Stdio};
use tempfile::TempDir;

const REPL_JS: &str = include_str!("./repl.js");
const SCRIPT_FILE_NAME: &str = "script.js";
const DEFAULT_NODE_BINARY: &str = "node";

// TODO randomize EOF for each call to repl
const DEFAULT_EOF: &[u8] = &[0, 1, 0];

type BuildCommand = dyn Fn(&Config, &str, &str) -> String;
#[derive(derive_builder::Builder, Default)]
#[builder(default, pattern = "owned")]
/// Configurating for [`Repl`]. Usually you will want to setup the REPL context by importing some modules
/// and doing some setup. Then maybe, run some teardown code after the REPL closes.
/// Do this by giving JavaScript strings to [`Config::imports`], [`Config::before`], and [`Config::after`] fields.
///
/// The Node.js script will look something like:
///
/// ```js
/// // eval Config::imports
///
/// (async () => {
///     // eval Config::before
///
///    for await (const line of repl()) {
///         eval(line)
///    }
///
///    // eval Config::after
/// })()
/// ```
/// You will probably want to provide [`Config::path_to_node_modules`] so use can use npm
/// packages .
pub struct Config {
    /// JS imports
    pub imports: Vec<String>,
    /// Code that runs before the REPL in an async context. setup, etc.
    pub before: Vec<String>,
    /// Define and run the REPL.
    #[builder(default = "REPL_JS.to_string()")]
    pub repl_code: String,
    /// Code that runs after the REPL. teardown, etc.
    /// Run in revers order.
    pub after: Vec<String>,
    /// Name of the file within which the REPL is run.
    #[builder(default = "SCRIPT_FILE_NAME.to_string()")]
    script_file_name: String,
    /// A function that constructs the shell script which runs the REPL.
    /// It is passed the config, the directory the REPL is run from, and the full path to the `script_file_name` file.
    /// Result looks like: `NODE_PATH=../node_modules /path/to/nodejs_binary /path/to/tmp/repl_script.js`.
    build_command: Option<Box<BuildCommand>>,
    /// A list paths that will be copied into the [`tempfile::TempDir`] alongside the REPL script.
    /// Useful for importing custom code.
    pub copy_dirs: Vec<String>,
    /// Path to a node_modules directory which node will use.
    pub path_to_node_modules: Option<String>,
    /// Path to node binary.
    #[builder(default = "DEFAULT_NODE_BINARY.to_string()")]
    node_binary: String,
    /// Delimiter used to signal end of a single loop in the REPL.
    #[builder(default = "DEFAULT_EOF.to_vec()")]
    eof: Vec<u8>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("imports", &self.imports)
            .field("before", &self.before)
            .field("repl_code", &self.repl_code)
            .field("after", &self.after)
            .field("script_file_name", &self.script_file_name)
            //.field("build_command", &self.build_command)
            .field("copy_dirs", &self.copy_dirs)
            .field("path_to_node_modules", &self.path_to_node_modules)
            .field("node_binary", &self.node_binary)
            .field("eof", &self.eof)
            .finish()
    }
}

impl Config {
    /// Build a default [`Config`].
    pub fn build() -> Result<Self> {
        Ok(ConfigBuilder::default().build()?)
    }
    /// Start Node.js and return [`Repl`].
    pub fn start(&self) -> Result<Repl> {
        let (dir, mut child) = run_code(self)?;
        Ok(Repl {
            dir,
            stdin: child.stdin.take().unwrap(),
            stdout: child.stdout.take().unwrap().bytes(),
            child,
            eof: self.eof.clone(),
        })
    }

    fn build_script(&self) -> String {
        let import_str = self.imports.join(";\n");
        let before_str = self.before.join(";\n");
        let after_str: Vec<String> = self.after.clone().into_iter().rev().collect();
        let after_str = after_str.join(";\n");
        format!(
            "
{import_str}
(async () => {{
{before_str}
  {}
  await repl();
{after_str}
}})();",
            self.repl_code
        )
    }
}

fn default_build_command(conf: &Config, _working_dir: &str, path_to_script: &str) -> String {
    let node_env = conf
        .path_to_node_modules
        .as_ref()
        .map(|p| format!("NODE_PATH={p}"))
        .unwrap_or_default();

    format!("{} {} {path_to_script}", node_env, conf.node_binary)
}

fn run_code(conf: &Config) -> Result<(TempDir, async_process::Child)> {
    let working_dir = tempfile::tempdir()?;

    let script_path = working_dir.path().join(&conf.script_file_name);
    let script_file = File::create(&script_path)?;

    write!(&script_file, "{}", &conf.build_script())?;

    let working_dir_path = working_dir.path().display().to_string();
    for dir in &conf.copy_dirs {
        let dir_cp_cmd = Command::new("cp")
            .arg("-r")
            .arg(dir)
            .arg(&working_dir_path)
            .output()?;
        if dir_cp_cmd.status.code() != Some(0) {
            return Err(Error::CommandFailed(
                dir_cp_cmd.status.code(),
                format!(
                    "failed to copy dir [{dir}] to [{working_dir_path}] got stderr: {}",
                    String::from_utf8_lossy(&dir_cp_cmd.stderr),
                ),
            ));
        }
    }
    let script_path_str = script_path.display().to_string();

    let cmd = match &conf.build_command {
        Some(func) => func(conf, &working_dir_path, &script_path_str),
        None => default_build_command(conf, &working_dir_path, &script_path_str),
    };
    Ok((
        working_dir,
        async_process::Command::new("sh")
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("-c")
            .arg(cmd)
            .spawn()?,
    ))
}

/// Interface to the Node.js REPL. Send code with [`Repl::run`], stop it with [`Repl::stop`].
#[derive(Debug)]
pub struct Repl {
    /// Needs to be held until the working directory should be dropped.
    pub dir: TempDir,
    /// stdin to the Node.js process.
    pub stdin: async_process::ChildStdin,
    /// stdout from the Node.js process.
    pub stdout: Bytes<async_process::ChildStdout>,
    /// Handle to the running Node.js process.
    pub child: async_process::Child,
    /// The delimiter used to end one read-eval-print-loop
    pub eof: Vec<u8>,
}

impl Repl {
    /// Run some JavaScript. Returns whatever is through Node's `stdout`.
    pub async fn run(&mut self, code: &str) -> Result<Vec<u8>> {
        let code = [
            b";(async () =>{\n",
            code.as_bytes(),
            b"; process.stdout.write('",
            &self.eof,
            b"');",
            b"})();",
        ]
        .concat();
        self.stdin.write_all(&code).await?;
        Ok(pull_result_from_stdout(&mut self.stdout, &self.eof).await)
    }

    /// Stop the REPL.
    pub async fn stop(&mut self) -> Result<Vec<u8>> {
        self.run("queue.done()'").await
    }
}

async fn pull_result_from_stdout(stdout: &mut Bytes<ChildStdout>, eof: &[u8]) -> Vec<u8> {
    let mut buff = vec![];
    while let Some(Ok(b)) = stdout.next().await {
        buff.push(b);
        if buff.ends_with(eof) {
            buff.truncate(buff.len() - eof.len());
            break;
        }
    }
    buff
}

#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum Error {
    #[error("cp command failed: code {0:?} msg: {1}")]
    CommandFailed(Option<i32>, String),
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Ut8Error: {0}")]
    Utf8Error(#[from] FromUtf8Error),
    #[error("serde_json::Error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Error building config: {0}")]
    ConfigBuilderError(#[from] ConfigBuilderError),
}
type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod test {
    use super::*;
    #[tokio::test]
    async fn read_eval_print_macro_works() -> Result<()> {
        let mut context: Repl = Config::build()?.start()?;
        let result = context.run("console.log('Hello, world!');").await?;
        assert_eq!(result, b"Hello, world!\n");
        let result = context
            .run(
                "
a = 66;
b = 7 + a;
c = 77;
process.stdout.write(`${b}`);
",
            )
            .await?;
        assert_eq!(result, b"73");
        let result = context.run("process.stdout.write(`${c}`)").await?;
        assert_eq!(result, b"77");

        let _result = context.stop().await?;
        let _ = context.child.output().await?;
        Ok(())
    }
}
