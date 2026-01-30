//! As this crate is intended for testing. I found it useful to include a few things related to
//! integration testing.

use std::process::Command;

#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum Error {
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Ut8Error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
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

/// Intialize logging
pub fn log() {
    static START_LOGS: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    START_LOGS.get_or_init(|| {
        use tracing_subscriber::{
            EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _,
        };
        let env_filter = EnvFilter::from_default_env(); // Reads `RUST_LOG` environment variable

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
