use futures_lite::{io::Bytes, AsyncReadExt, AsyncWriteExt, StreamExt};

use std::{fs::File, io::Write, process::Command, string::FromUtf8Error};

use async_process::{ChildStdout, Stdio};
use tempfile::TempDir;

pub static REPL_JS: &str = include_str!("./repl.js");

// TODO randomize EOF for each call to repl
static DEFAULT_EOF: &[u8] = &[0, 1, 0];
static SCRIPT_FILE_NAME: &str = "script.js";

static DEFAULT_NODE_BINARY: &str = "node";

#[derive(derive_builder::Builder, Default)]
#[builder(default, pattern = "owned")]
pub struct ReplConf {
    /// define and run the repl
    #[builder(default = "REPL_JS.to_string()")]
    repl_code: String,
    /// the name of the file within which the repl is run
    #[builder(default = "SCRIPT_FILE_NAME.to_string()")]
    script_file_name: String,
    /// A function that constructs the shell script which runs the repl.
    /// It is passed the directory the reply is run from, and the full path to the `script_file_name` file.
    /// By default the function creates the command `/path/to/nodejs /path/to/repl_script.js`.
    build_command: Option<Box<dyn Fn(&ReplConf, &str, &str) -> String>>,
    /// a list paths that will be copied into the directory alongside the script.
    copy_dirs: Vec<String>,
    /// path to a node_modules directory which node will use
    path_to_node_modules: Option<String>,
    /// path to node binary
    #[builder(default = "DEFAULT_NODE_BINARY.to_string()")]
    node_binary: String,
    #[builder(default = "DEFAULT_EOF.to_vec()")]
    eof: Vec<u8>,
}

impl ReplConf {
    pub fn start(&self) -> Result<JsContext> {
        let (dir, mut child) = run_code(&self)?;
        Ok(JsContext {
            dir,
            stdin: child.stdin.take().unwrap(),
            stdout: child.stdout.take().unwrap().bytes(),
            child,
            eof: self.eof.clone(),
        })
    }
}

pub struct JsContext {
    /// needs to be held until the working directory should be dropped
    pub dir: TempDir,
    pub stdin: async_process::ChildStdin,
    pub stdout: Bytes<async_process::ChildStdout>,
    pub child: async_process::Child,
    pub eof: Vec<u8>,
}

impl JsContext {
    pub async fn repl(&mut self, code: &str) -> Result<Vec<u8>> {
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
}

fn default_build_command(conf: &ReplConf, _working_dir: &str, path_to_script: &str) -> String {
    let node_env = conf
        .path_to_node_modules
        .as_ref()
        .map(|p| format!("NODE_ENV={p}"))
        .unwrap_or(Default::default());

    format!("{} {} {path_to_script}", node_env, conf.node_binary)
}

fn run_code(conf: &ReplConf) -> Result<(TempDir, async_process::Child)> {
    let working_dir = tempfile::tempdir()?;

    let script_path = working_dir.path().join(&conf.script_file_name);
    let script_file = File::create(&script_path)?;

    write!(&script_file, "{}", &conf.repl_code)?;

    let working_dir_path = working_dir.path().display().to_string();
    for dir in &conf.copy_dirs {
        let dir_cp_cmd = Command::new("cp")
            .arg("-r")
            .arg(&dir)
            .arg(&working_dir_path)
            .output()?;
        if dir_cp_cmd.status.code() != Some(0) {
            return Err(Error::TestError(format!(
                "failed to copy dir [{dir}] to [{working_dir_path}] got stderr: {}",
                String::from_utf8_lossy(&dir_cp_cmd.stderr),
            )));
        }
    }
    let script_path_str = script_path.display().to_string();

    let cmd = match &conf.build_command {
        Some(func) => {
            //let f = &func.as_ref();
            func(&conf, &working_dir_path, &script_path_str)
        }
        None => default_build_command(conf, &working_dir_path, &script_path_str),
    };
    //let cmd = build_command(&working_dir_path, &script_path_str);
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
pub enum Error {
    #[error("Problem in tests: {0}")]
    TestError(String),
    #[error("IoError")]
    IoError(#[from] std::io::Error),
    #[error("Ut8Error")]
    Utf8Error(#[from] FromUtf8Error),
    #[error("serde_json::Error")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("repl builder error")]
    ReplConfBuilerError(#[from] ReplConfBuilderError),
}
type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod test {
    use super::*;
    #[tokio::test]
    async fn read_eval_print_macro_works() -> Result<()> {
        let mut context = ReplConfBuilder::default().build()?.start()?;
        let result = context.repl("process.stdout.write('fooo6!');").await?;
        assert_eq!(result, b"fooo6!");
        let result = context
            .repl(
                "
a = 66;
b = 7 + a;
c = 77;
process.stdout.write(`${b}`);
",
            )
            .await?;
        assert_eq!(result, b"73");
        let result = context.repl("process.stdout.write(`${c}`)").await?;
        assert_eq!(result, b"77");

        let _result = context.repl("queue.done();").await?;
        let _ = context.child.output().await?;
        Ok(())
    }
}
