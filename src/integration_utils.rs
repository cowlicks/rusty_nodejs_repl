//! Useful things for wrting integration tests
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
