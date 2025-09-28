//! JavaScript & Rust socket communication

use std::future::Future;

use crate::{Error, Repl, Result};

use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    spawn,
    task::JoinHandle,
};

/// "0" uses a random free port
pub const DEFAULT_PORT: &str = "0";
/// Default hostname used for rs js stream
pub const LOOPBACK: &str = "127.0.0.1";
/// Default name of the JavaScript variable that holds the stream
pub const DEFAULT_JS_SOCKET_NAME: &str = "socket";
/// Default code run after js socket
pub const DEFAULT_JS_AFTER_SOCKET_CODE: &str = "(s) => {
    output = (x) => {
        s.write(x);
    }
}";
/// Default # of milliseconds that wait macro waits
pub const DEFAULT_WAIT_MILLIS: u64 = 100;

#[macro_export]
/// sleep().await for the given # of millis. Defaults to 100ms.
/// Useful for async testing .
macro_rules! wait {
    ($millis:expr) => {
        tokio::time::sleep(std::time::Duration::from_millis($millis)).await;
    };
    () => {
        wait!(rusty_nodejs_repl::pipe::DEFAULT_WAIT_MILLIS)
    };
}
pub use wait;

/// Configuration for a Rust to Js stream. Responsible for creating a [`TcpStream`] and the code
/// that must run within the repl to set it up
#[derive(derive_builder::Builder, Debug, Default)]
#[builder(derive(Debug), pattern = "owned")]
pub struct IoConfig {
    /// The port to use. Defaults to "0" which casues a random port to be used
    #[builder(default = "DEFAULT_PORT.to_string()")]
    pub rs_listener_port: String,
    /// The hostname the socket connects to
    #[builder(default = "LOOPBACK.to_string()")]
    pub hostname: String,
    /// The name of the JavaScript variable that holds the stream
    #[builder(default = "DEFAULT_JS_SOCKET_NAME.to_string()")]
    pub js_socket_name: String,
    /// Code run after the socket is created, which takes the socket as the argument.
    /// Typically this would create the "`output`" JavaScript function that is used to send data
    /// from JavaScript to Rust.
    #[builder(default = "DEFAULT_JS_AFTER_SOCKET_CODE.to_string()")]
    pub js_after_socket_code: String,
}

impl IoConfig {
    /// Returns a Future that resolves to a socket ([`TcpStream`]) which reads data from
    /// JavaScript; as well as JavaScript setup and teardown code.
    ///
    /// In Rust we create a server, and a future that resolves to the first client that connects.
    /// The JavaScript setup code, when run, connects to the Rust server; teardown code disconnects
    /// from the server.
    pub async fn start_server_and_make_js_code(
        &self,
    ) -> Result<(impl Future<Output = Result<TcpStream>>, (String, String))> {
        let Self {
            rs_listener_port,
            hostname,
            ..
        } = self;

        let listener = TcpListener::bind(format!("{hostname}:{rs_listener_port}")).await?;
        let shared_port = listener.local_addr()?.port();
        let out: JoinHandle<Result<TcpStream>> =
            spawn(async move { Ok(listener.accept().await?.0) });

        let stream_fut = async move { out.await.map_err(Error::RsSocketFail)? };
        let setup_code = Self::make_js_setup_code(self, shared_port);
        let teardown_code = Self::make_js_teardown_code(self);
        Ok((stream_fut, (setup_code, teardown_code)))
    }
    fn make_js_setup_code(
        Self {
            js_after_socket_code,
            hostname,
            js_socket_name,
            ..
        }: &Self,
        shared_port: u16,
    ) -> String {
        format!(
            "
// Connect to the port and define socket
{js_socket_name} = require('net').connect('{shared_port}', '{hostname}');
///
;await ({js_after_socket_code})({js_socket_name});
"
        )
    }

    fn make_js_teardown_code(Self { js_socket_name, .. }: &Self) -> String {
        format!(
            "// close socket
{js_socket_name}.destroy()"
        )
    }
}

fn js_code(
    shared_port: &str,
    IoConfig {
        hostname,
        js_socket_name,
        js_after_socket_code,
        ..
    }: &IoConfig,
) -> String {
    format!(
        "
// Connect to the port and define socket
{js_socket_name} = require('net').connect('{shared_port}', '{hostname}');
///
;await ({js_after_socket_code})({js_socket_name});
"
    )
}

/// create a stream connecting rust and js
pub async fn rust_js_stream(repl: &mut Repl, conf: &IoConfig) -> Result<TcpStream> {
    let listener =
        TcpListener::bind(format!("{}:{}", conf.hostname, conf.rs_listener_port)).await?;
    let shared_port = format!("{}", listener.local_addr()?.port());
    let out: JoinHandle<Result<TcpStream>> = spawn(async move { Ok(listener.accept().await?.0) });

    let js_setup_code = js_code(&shared_port, conf);
    let _ = repl.run(&js_setup_code).await?;
    out.await.map_err(Error::RsSocketFail)?
}

/// Read from the provided [`TcpStream`] until we read the value of the `eof` argument`. Return all
/// data read before `eof`.
pub async fn pull_result_from_tcp(stream: &mut TcpStream, eof: &[u8]) -> Result<Vec<u8>> {
    let mut buff = vec![];
    let mut byte = [0u8; 1];

    loop {
        let _n_bytes_read = stream.read_exact(&mut byte).await?;
        buff.push(byte[0]);
        if buff.ends_with(eof) {
            buff.truncate(buff.len() - eof.len());
            break;
        }
    }
    Ok(buff)
}

#[cfg(test)]
mod test {
    use crate::Config;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn rust_to_js_stream() -> Result<()> {
        // create the stream. On the JS end, read from the socket and send it back
        let conf = IoConfigBuilder::default()
            .js_after_socket_code(
                "(s) => {
                s.on('data', (chunk) => {
                    s.write(`echo ${chunk.toString()}`);
                })
            }"
                .to_string(),
            )
            .build()?;

        let mut repl: Repl = Config::build()?.start().await?;
        let mut stream = rust_js_stream(&mut repl, &conf).await?;
        stream.write_all(b"hello and back").await?;
        let mut out = vec![0; 19];
        stream.read_exact(&mut out).await?;
        assert_eq!(out, b"echo hello and back");
        Ok(())
    }
    mod macros {
        //! Putting this macro in it's own module so that we can test that it is not implicitly
        //! relying on context
        #[tokio::test]
        async fn wait_test() {
            wait!(1);
        }
    }

    #[tokio::test]
    async fn bar() -> Result<()> {
        // create the stream. On the JS end, read from the socket and send it back
        let conf = IoConfigBuilder::default().build()?;
        let (out, (setup, _teardown)) = conf.start_server_and_make_js_code().await?;

        let mut repl: Repl = Config::build()?.start().await?;

        let eof = "abc";
        let _ = repl.run(&setup).await?;
        let mut stream = out.await?;

        let x = repl
            .run(format!(
                "
console.log('24');
output('69{eof}');
"
            ))
            .await?;
        assert_eq!(x, b"24\n");
        let result = pull_result_from_tcp(&mut stream, eof.as_bytes()).await?;
        assert_eq!(result, b"69");
        Ok(())
    }
}
