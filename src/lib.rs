/*!
This crate lets you run arbitrary code in a  Node.js REPL from Rust.
Use [`Config`] to setup the REPL and use [`Repl`] to interact with it.
```rust
# tokio_test::block_on(async {
# use rusty_nodejs_repl::{Repl, Config, Error};
let mut repl: Repl = Config::build()?.start().await?;
let result = repl.run("output('Hello, world!');").await?;
assert_eq!(result, b"Hello, world!");
repl.stop().await?;
# Ok::<(),Error>(())
# }).unwrap();
```
The REPL is run in it's own [`tempfile::TempDir`]. So any files created alongside it will be cleaned up on exit.
*/
#![warn(missing_debug_implementations, refining_impl_trait, missing_docs)]

mod error;
#[cfg(feature = "integration_utils")]
pub mod integration_utils;
pub mod pipe;

use async_process::{ChildStdout, Stdio};
use futures_lite::{AsyncReadExt, AsyncWriteExt, Stream, StreamExt, io::Bytes};
use std::{fs::File, io::Write, process::Command, time::Duration};
use tempfile::TempDir;
use tokio::{net::TcpStream, time::timeout};
use tracing::error;

use crate::pipe::{IoConfig, IoConfigBuilder, pull_result_from_socket};
pub use error::{Error, Result};

const REPL_JS: &str = include_str!("./repl.js");
const SCRIPT_FILE_NAME: &str = "script.js";
const DEFAULT_NODE_BINARY: &str = "node";

// TODO randomize EOF for each call to repl
const DEFAULT_EOF: &[u8] = &[0, 1, 0];
const DEFAULT_READBUFF_TIMEOUT_MS: u64 = 100;
const DEFAULT_READBUFF_TIMEOUT: Duration = Duration::from_millis(DEFAULT_READBUFF_TIMEOUT_MS);

const REPL_READY: &str = "repl_ready";

type BuildCommand = dyn Fn(&Config, &str, &str) -> String;

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
#[derive(derive_builder::Builder, Default)]
#[builder(default, pattern = "owned")]
pub struct Config {
    /// JS imports
    pub imports: Vec<String>,
    /// Code that runs before the REPL in an async context. setup, etc.
    pub before: Vec<String>,
    /// Define and run the REPL.
    #[builder(default = "REPL_JS.to_string()")]
    pub repl_code: String,
    /// Code that runs after the REPL. teardown, etc.
    /// Run in reverse order.
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
    /// Configuration for creating a socket connecting the Rust and JavaScript process
    #[builder(default = "IoConfigBuilder::default().build().unwrap()")]
    io_config: IoConfig,
}

/// turn a rust vec like vec![1, 2, 3] into "Buffer.from([1, 2, 3])"
fn fmt_rs_vec_u8_as_js_buf(bytes: &[u8]) -> String {
    let nums: Vec<String> = bytes.iter().map(|x| x.to_string()).collect();
    let s = nums.join(", ");
    format!("Buffer.from([{s}])")
}

fn add_semis_to_ensure_js_statement(statements: &[String]) -> Vec<String> {
    statements.iter().map(|s| format!("{s};")).collect()
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
    pub async fn start(&mut self) -> Result<Repl> {
        #[cfg(feature = "socket")]
        let socket_future = {
            let (socket_future, (js_setup_code, teardown_code)) =
                self.io_config.start_server_and_make_js_code().await?;

            self.before.push(js_setup_code);
            self.after.push(teardown_code);
            socket_future
        };
        let (dir, mut child) = run_code(self)?;
        let stdin = child.stdin.take().unwrap();
        let mut stderr = child.stderr.take().unwrap().bytes();
        let mut stdout = child.stdout.take().unwrap().bytes();
        let read_res = pull_result_from_stdout(&mut stdout, &self.eof).await;
        if !String::from_utf8_lossy(&read_res).contains(REPL_READY) {
            return Err(Error::FailedToStart(child));
        }
        let errs = read_with_timeout(&mut stderr, DEFAULT_READBUFF_TIMEOUT).await;
        if !errs.is_empty() {
            error!("{}", String::from_utf8_lossy(&errs));
            return Err(Error::RunError);
        }

        Ok(Repl {
            dir,
            stdin,
            stderr,
            stdout,
            child,
            eof: self.eof.clone(),
            #[cfg(feature = "socket")]
            socket: socket_future.await?,
        })
    }

    fn build_script(&self) -> String {
        let import_str = add_semis_to_ensure_js_statement(&self.imports).join("\n");
        let before_str = add_semis_to_ensure_js_statement(&self.before).join("\n");
        let after_str: Vec<String> = self.after.clone().into_iter().rev().collect();
        let after_str = after_str.join(";\n");
        let eof_buf = fmt_rs_vec_u8_as_js_buf(&self.eof);
        let out = format!(
            "
// start imports
{import_str}
// end imports
(async () => {{
// start before_str
{before_str}
// end before_str

  {}
  ; process.stdout.write('{REPL_READY}');
  ; process.stdout.write({});
  await repl();
// start after_str
{after_str}
// end after_str
}})();",
            self.repl_code, eof_buf,
        );

        out
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
    let proc = async_process::Command::new("sh")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("-c")
        .arg(cmd)
        .spawn()?;
    Ok((working_dir, proc))
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
    /// stderr from the Node.js process.
    pub stderr: Bytes<async_process::ChildStderr>,
    /// Handle to the running Node.js process.
    pub child: async_process::Child,
    /// The delimiter used to end one read-eval-print-loop
    pub eof: Vec<u8>,
    /// IO socket
    #[cfg(feature = "socket")]
    pub socket: TcpStream,
}

impl Repl {
    /// get contents of stderr
    pub async fn drain_stderr(&mut self) -> Result<Vec<u8>> {
        Ok(read_with_timeout(&mut self.stderr, DEFAULT_READBUFF_TIMEOUT).await)
    }
    /// get contents of stdout
    pub async fn drain_stdout(&mut self) -> Result<Vec<u8>> {
        Ok(read_with_timeout(&mut self.stdout, DEFAULT_READBUFF_TIMEOUT).await)
    }

    // TODO: add a way to rename `output`.
    /// Run some JavaScript. Return's a [`Vec<u8>`] containing whatever is sent through the
    /// "`output`" function in the JavaScript process. This is like [`Repl::run`] except it gets
    /// the result from TCP socket instead of stdout.
    #[cfg(feature = "socket")]
    pub async fn run<S: AsRef<str>>(&mut self, code: S) -> Result<Vec<u8>> {
        let code = code.as_ref();
        let code = [
            b";(async () =>{\n",
            code.as_bytes(),
            b"; output('",
            &self.eof,
            b"');",
            b"})();",
        ]
        .concat();
        self.stdin.write_all(&code).await?;
        let res = pull_result_from_socket(&mut self.socket, &self.eof).await;
        if let Err(e) = &res
            && let Error::IoError(ioerr) = e
            && std::io::ErrorKind::UnexpectedEof == ioerr.kind()
        {
            // log stderr
            let stderr = self.drain_stderr().await?;
            let stdout = self.drain_stdout().await?;
            let stderr = String::from_utf8_lossy(&stderr);
            let stdout = String::from_utf8_lossy(&stdout);
            eprintln!(
                "Repl.run failed.
>>>>>>>>>> STDOUT >>>>>>>>>>
stdout:\n{stdout}
<<<<<<<< END STDOUT <<<<<<<<
>>>>>>>>>> STDERR >>>>>>>>>>
stderr:\n{stderr}
<<<<<<<< END STDERR <<<<<<<<
"
            );
            Err(Error::RunTcpError(stderr.to_string()))
        } else {
            res
        }
    }

    /// Print stdout & stderr and return them.
    pub async fn print(&mut self) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        let stderr = self.drain_stderr().await?;
        if !stderr.is_empty() {
            println!("stderr: {}", String::from_utf8(stderr.clone())?);
        }
        let stdout = self.drain_stdout().await?;
        if !stdout.is_empty() {
            println!("stdout: {}", String::from_utf8(stdout.clone())?);
        }
        if stderr.is_empty() && stdout.is_empty() {
            Ok(None)
        } else {
            Ok(Some((stdout, stderr)))
        }
    }

    /// Print JS stdout & stderr until there is nothing left to print
    pub async fn print_until_settled(&mut self) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        while self.print().await?.is_some() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }
    /// Run some JavaScript. Returns whatever is through Node's `stdout`.
    pub async fn str_run<S: AsRef<str>>(&mut self, code: S) -> Result<String> {
        Ok(String::from_utf8(self.run(code).await?)?)
    }

    #[cfg(feature = "serde")]
    /// Run some JavaScript. Deserialize stdout into `T`.
    pub async fn json_run_old<T: serde::de::DeserializeOwned, S: AsRef<str>>(
        &mut self,
        code: S,
    ) -> Result<T> {
        let result = self.run(code).await?;
        Ok(serde_json::from_str(&String::from_utf8(result)?)?)
    }

    #[cfg(feature = "serde")]
    /// Run some JavaScript. Deserialize stdout into `T`.
    pub async fn json_run<T: serde::de::DeserializeOwned, S: AsRef<str>>(
        &mut self,
        code: S,
    ) -> Result<T> {
        let result = self.run(code).await?;
        Ok(serde_json::from_str(&String::from_utf8(result)?)?)
    }

    #[cfg(feature = "serde")]
    /// Run some JavaScript. Deserialize stdout into `T`.
    pub async fn get_name<T: serde::de::DeserializeOwned, S: std::fmt::Display>(
        &mut self,
        name: S,
    ) -> Result<T> {
        self.json_run(format!("outputJson(await {name})")).await
    }

    /// Stop the REPL.
    pub async fn stop(&mut self) -> Result<Vec<u8>> {
        self.run("queue.done();").await
    }
}

async fn read_with_timeout<T, E, S: Stream<Item = std::result::Result<T, E>> + Unpin>(
    stream: &mut S,
    duration: Duration,
) -> Vec<T> {
    let mut buff = vec![];
    while let Some(Ok(b)) = timeout(duration, stream.next()).await.unwrap_or_default() {
        buff.push(b);
    }
    buff
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

#[cfg(test)]
mod test {
    use super::*;
    #[tokio::test]
    async fn read_eval_print_works() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result = repl.run("output('Hello, world!');").await?;
        assert_eq!(result, b"Hello, world!");
        let result = repl
            .run(
                "
a = 66;
b = 7 + a;
c = 77;
output(`${b}`);
",
            )
            .await?;
        assert_eq!(result, b"73");
        let result = repl.run("output(`${c}`)").await?;
        assert_eq!(result, b"77");

        let _result = repl.stop().await?;
        let _ = repl.child.output().await?;
        Ok(())
    }

    #[cfg(feature = "socket")]
    #[tokio::test]
    async fn test_run_tcp() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result = repl.run("output('Hello, world!');").await?;
        assert_eq!(result, b"Hello, world!");
        let result = repl
            .run(
                "
a = 66;
b = 7 + a;
c = 77;
output(`${b}`);
",
            )
            .await?;
        assert_eq!(result, b"73");
        let result = repl.run("output(`${c}`)").await?;
        assert_eq!(result, b"77");

        let _result = repl.stop().await?;
        let _ = repl.child.output().await?;
        Ok(())
    }

    #[tokio::test]
    async fn run_with_syntax_error() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result = repl.run("output(syntax error here").await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn config_imports_work() -> Result<()> {
        let mut conf = ConfigBuilder::default()
            .imports(vec!["var path = require('path')".to_string()])
            .build()?;
        let mut repl = conf.start().await?;
        let result = repl
            .str_run("output(path.basename('/foo/bar.txt'))")
            .await?;
        assert_eq!(result, "bar.txt");
        Ok(())
    }

    #[tokio::test]
    async fn config_before_runs() -> Result<()> {
        let mut conf = ConfigBuilder::default()
            .before(vec!["globalThis.setupValue = 42".to_string()])
            .build()?;
        let mut repl = conf.start().await?;
        let result = repl.str_run("output(`${setupValue}`)").await?;
        assert_eq!(result, "42");
        Ok(())
    }

    #[tokio::test]
    async fn str_run_returns_string() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result: String = repl.str_run("output('hello')").await?;
        assert_eq!(result, "hello");
        Ok(())
    }

    #[tokio::test]
    async fn empty_output() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result = repl.run("output('')").await?;
        assert_eq!(result, b"");
        Ok(())
    }

    #[tokio::test]
    async fn binary_data_through_socket() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result = repl.run("output(Buffer.from([0, 255, 128]))").await?;
        assert_eq!(result, vec![0u8, 255, 128]);
        Ok(())
    }

    #[test]
    fn test_fmt_rs_vec_u8_as_js_buf() {
        assert_eq!(
            fmt_rs_vec_u8_as_js_buf(&[1, 2, 3]),
            "Buffer.from([1, 2, 3])"
        );
        assert_eq!(fmt_rs_vec_u8_as_js_buf(&[]), "Buffer.from([])");
    }

    #[tokio::test]
    async fn custom_eof_delimiter() -> Result<()> {
        let mut conf = ConfigBuilder::default().eof(b"DONE".to_vec()).build()?;
        let mut repl = conf.start().await?;
        let result = repl.run("output('test')").await?;
        assert_eq!(result, b"test");
        Ok(())
    }

    #[cfg(feature = "serde")]
    #[tokio::test]
    async fn get_name_deserializes_js_value() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        repl.run("globalThis.testObj = { name: 'alice', age: 30 }")
            .await?;
        let result: serde_json::Value = repl.get_name("testObj").await?;
        assert_eq!(result["name"], "alice");
        assert_eq!(result["age"], 30);
        Ok(())
    }

    #[cfg(feature = "serde")]
    #[tokio::test]
    async fn json_run_deserializes_result() -> Result<()> {
        let mut repl: Repl = Config::build()?.start().await?;
        let result: Vec<i32> = repl.json_run("outputJson([1, 2, 3])").await?;
        assert_eq!(result, vec![1, 2, 3]);
        Ok(())
    }
}
