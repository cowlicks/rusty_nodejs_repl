use futures_lite::{io::Bytes, AsyncReadExt, AsyncWriteExt, StreamExt};

use std::{fs::File, io::Write, path::PathBuf, process::Command, string::FromUtf8Error};

use async_process::{ChildStdout, Stdio};
use tempfile::TempDir;

pub static _PATH_TO_DATA_DIR: &str = "src/js/data";
pub static LOOPBACK: &str = "127.0.0.1";
pub static REL_PATH_TO_NODE_MODULES: &str = "./js/node_modules";
pub static REL_PATH_TO_JS_DIR: &str = "./src/js";

static DEFAULT_EOF: &[u8] = &[0, 1, 0];
static SCRIPT_FILE_NAME: &str = "script.js";
pub static RUN_REPL_CODE: &str = r#"
const { repl } = require('./utils.js');
// start a read-eval-print-loop we use from rust
await repl();
"#;

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
}
pub type Result<T> = core::result::Result<T, Error>;

macro_rules! join_paths {
    ( $path:expr$(,)?) => {
        $path
    };
    ( $p1:expr,  $p2:expr) => {{
        let p = std::path::Path::new(&*$p1).join($p2);
        p.display().to_string()
    }};
    ( $p1:expr,  $p2:expr, $($tail:tt)+) => {{
        let p = std::path::Path::new($p1).join($p2);
        join_paths!(p.display().to_string(), $($tail)*)
    }};
}
pub(crate) use join_paths;

pub fn git_root() -> Result<String> {
    let x = Command::new("sh")
        .arg("-c")
        .arg("git rev-parse --show-toplevel")
        .output()?;
    Ok(String::from_utf8(x.stdout)?.trim().to_string())
}

pub fn path_to_js_dir() -> Result<PathBuf> {
    Ok(join_paths!(git_root()?, &REL_PATH_TO_JS_DIR).into())
}

pub fn path_to_node_modules() -> Result<PathBuf> {
    let p = join_paths!(git_root()?, &REL_PATH_TO_NODE_MODULES);
    Ok(p.into())
}

pub fn async_iiaf_template(async_body_str: &str) -> String {
    format!(
        "(async () => {{
{}
}})()",
        async_body_str
    )
}

fn build_command(_working_dir: &str, script_path: &str) -> String {
    format!(
        "NODE_PATH={} node {}",
        path_to_node_modules().unwrap().display(),
        script_path
    )
}
#[derive(derive_builder::Builder)]
#[builder(pattern = "owned")]
pub struct JsContext2 {
    /// needs to be held until the working directory should be dropped
    #[builder(default = "self.set_dir_default()?")]
    pub dir: TempDir,
    pub stdin: async_process::ChildStdin,
    pub stdout: Bytes<async_process::ChildStdout>,
    pub child: async_process::Child,
    pub eof: Vec<u8>,
}
impl JsContext2Builder {
    fn set_dir_default(&self) -> std::result::Result<TempDir, String> {
        match tempfile::tempdir() {
            Ok(tmp) => Ok(tmp),
            Err(e) => Err(format!("{e}")),
        }
    }
}

//#[derive(derive_builder::Builder)]
pub struct JsContext {
    /// needs to be held until the working directory should be dropped
    pub dir: TempDir,
    pub stdin: async_process::ChildStdin,
    pub stdout: Bytes<async_process::ChildStdout>,
    pub child: async_process::Child,
    pub eof: Vec<u8>,
}

impl JsContext {
    pub fn new() -> Result<Self> {
        Ok(run_js(
            &async_iiaf_template(RUN_REPL_CODE),
            vec![format!("{}/utils.js", path_to_js_dir()?.to_string_lossy())],
        )?)
    }

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

pub fn run_js(code_string: &str, copy_dirs: Vec<String>) -> Result<JsContext> {
    let (dir, mut child) = run_code(code_string, SCRIPT_FILE_NAME, build_command, copy_dirs)?;
    Ok(JsContext {
        dir,
        stdin: child.stdin.take().unwrap(),
        stdout: child.stdout.take().unwrap().bytes(),
        child,
        eof: DEFAULT_EOF.to_vec(),
    })
}

pub fn run_code(
    code_string: &str,
    script_file_name: &str,
    build_command: impl FnOnce(&str, &str) -> String,
    copy_dirs: Vec<String>,
) -> Result<(TempDir, async_process::Child)> {
    let working_dir = tempfile::tempdir()?;

    let script_path = working_dir.path().join(script_file_name);
    let script_file = File::create(&script_path)?;

    write!(&script_file, "{}", &code_string)?;

    let working_dir_path = working_dir.path().display().to_string();
    // copy dirs into working dir
    for dir in copy_dirs {
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
    let cmd = build_command(&working_dir_path, &script_path_str);
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

pub async fn pull_result_from_stdout(stdout: &mut Bytes<ChildStdout>, eof: &[u8]) -> Vec<u8> {
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
    async fn read_eval_print_macro_works() -> Result<()> {
        let mut context = JsContext::new()?;
        let result = context.repl("process.stdout.write('fooo6!');").await?;
        assert_eq!(result, b"fooo6!");
        let result = context
            .repl(
                "
a = 66;
b = 7 + a;
process.stdout.write(`${b}`);
",
            )
            .await?;
        assert_eq!(result, b"73");

        let _result = context.repl("queue.done();").await?;
        let _ = context.child.output().await?;
        Ok(())
    }
}
