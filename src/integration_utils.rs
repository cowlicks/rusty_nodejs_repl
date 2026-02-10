//! As this crate is intended for testing. I found it useful to include a few things related to
//! integration testing.

use std::process::{Command, Output};

#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum Error {
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Ut8Error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    #[error("Non-zero status code returned from command")]
    NonZeroStatusCodeFromCommand(String),
}

#[macro_export]
/// Join paths into one
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

/// Get the absolute path of the the top-level directory of the current git repository.
pub fn git_root() -> Result<String, Error> {
    let x = Command::new("sh")
        .arg("-c")
        .arg("git rev-parse --show-toplevel")
        .output()?;
    Ok(String::from_utf8(x.stdout)?.trim().to_string())
}

/// Turn [`std::process::Output`] into a an Error if the status code != 0.
pub fn check_cmd_output(out: Output) -> Result<Output, Error> {
    if out.status.code() != Some(0) {
        return Err(Error::NonZeroStatusCodeFromCommand(format!(
            "comand output status was not zero. Got:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        )));
    }
    Ok(out)
}

/// Run's `make` with the given argument in the given directory.
/// The direcory should be relative to the root of the `git` repo.
/// `make` is run with a lock-file to avoid problems with it being run in parallel.
pub fn run_make(makefile_directory: &str, arg: &str) -> Result<Output, Error> {
    let path = join_paths!(git_root()?, makefile_directory);
    let cmd = format!("cd {path} && flock make.lock make {arg} && rm -f make.lock ");
    let cmd_res = Command::new("sh").arg("-c").arg(cmd).output()?;
    let out = check_cmd_output(cmd_res)?;
    Ok(out)
}

/// Intialize logging
pub fn log() {
    static START_LOGS: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    START_LOGS.get_or_init(|| {
        use tracing_subscriber::{
            EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _,
        };
        let env_filter = EnvFilter::from_default_env(); // Reads `RUST_LOG` environment variable
        //tracing_subscriber::fmt()
        //    .with_target(true)
        //    .with_line_number(true)
        //    // print when instrumented funtion enters
        //    //.with_span_events(FmtSpan::ENTER | FmtSpan::EXIT)
        //    .with_file(true)
        //    .with_env_filter(EnvFilter::from_default_env()) // Reads `RUST_LOG` environment variable
        //    .without_time()
        //    .init();

        // Create the hierarchical layer from tracing_tree
        let tree_layer = tracing_tree::HierarchicalLayer::new(2) // 2 spaces per indent level
            .with_targets(true)
            .with_bracketed_fields(true)
            .with_indent_lines(true)
            .with_thread_ids(false)
            .with_thread_names(true)
            //.with_span_modes(true)
            ;

        tracing_subscriber::registry()
            .with(env_filter)
            .with(tree_layer)
            .init();
    });
}
