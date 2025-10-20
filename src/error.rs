#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Rust side error creating rs to js socket: {0}")]
    RsSocketFail(tokio::task::JoinError),
    #[error("Error building rust to javascript stream")]
    RsJsStreamConfBad(#[from] crate::pipe::IoConfigBuilderError),
    #[error("cp command failed: code {0:?} msg: {1}")]
    CommandFailed(Option<i32>, String),
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Ut8Error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    #[cfg(feature = "serde")]
    #[error("serde_json::Error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Error building config: {0}")]
    ConfigBuilderError(#[from] crate::ConfigBuilderError),
    #[error("Repl failed to start. This could be an issue with imports: {0:?}")]
    FailedToStart(async_process::Child),
    #[error("Repl got an error running your code")]
    RunError,
    #[cfg(feature = "integration_utils")]
    #[error("Error from integration utils")]
    IntegrationUtils(#[from] crate::integration_utils::Error),
    #[error("Repl.run_tcp error {0}")]
    RunTcpError(String),
}

/// Alias of [`std::error::Error`] to use our own [`Error`]
pub type Result<T> = core::result::Result<T, Error>;
