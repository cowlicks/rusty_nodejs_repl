//! Utils for creating a stream from rust to javascript

use crate::{Error, Repl, Result};

use tokio::{
    net::{TcpListener, TcpStream},
    spawn,
    task::JoinHandle,
};

/// "0" uses a random free port
pub const DEFAULT_PORT: &str = "0";
/// Default hostname used for rs js stream
pub const LOOPBACK: &str = "127.0.0.1";
/// Default name of the javascript variable that holds the stream
pub const DEFAULT_JS_SOCKET_NAME: &str = "socket";
/// Default code run after js socket
pub const DEFAULT_JS_AFTER_SOCKET_CODE: &str = "(() => {})";
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

/// Configuration for a Rust to Js stream
#[derive(derive_builder::Builder, Debug)]
#[builder(derive(Debug), pattern = "owned")]
pub struct IoConfig {
    #[builder(default = "DEFAULT_PORT.to_string()")]
    /// The port to use. Defaults to "0" which casues a random port to be used
    rs_listener_port: String,
    #[builder(default = "LOOPBACK.to_string()")]
    /// The hostname used
    hostname: String,
    #[builder(default = "DEFAULT_JS_SOCKET_NAME.to_string()")]
    /// The name of the javascript variable that holds the stream
    js_socket_name: String,
    #[builder(default = "DEFAULT_JS_AFTER_SOCKET_CODE.to_string()")]
    /// code run after the socket
    js_after_sockect_code: String,
}

fn js_code(
    shared_port: &str,
    IoConfig {
        hostname,
        js_socket_name,
        js_after_sockect_code,
        ..
    }: &IoConfig,
) -> String {
    format!(
        "
// Connect to the port and define socket
{js_socket_name} = require('net').connect('{shared_port}', '{hostname}');
///
;await ({js_after_sockect_code})({js_socket_name});
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

#[cfg(test)]
mod test {
    use crate::Config;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn rust_to_js_stream() -> Result<()> {
        // create the stream. On the JS end, read from the socket and send it back
        let conf = IoConfigBuilder::default()
            .js_after_sockect_code(
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
        //! relying on contect
        #[tokio::test]
        async fn wait_test() {
            wait!(1);
        }
    }

    #[tokio::test]
    async fn foo() -> Result<()> {
        // create the stream. On the JS end, read from the socket and send it back
        let conf = IoConfigBuilder::default()
            .js_after_sockect_code(
                "(s) => {
    output = (x) => {
        s.write(x);
    }
}"
                .to_string(),
            )
            .build()?;

        let mut repl: Repl = Config::build()?.start().await?;
        //let mut stream = rust_js_stream(&mut repl, &conf).await?;

        let listener =
            TcpListener::bind(format!("{}:{}", conf.hostname, conf.rs_listener_port)).await?;
        let shared_port = format!("{}", listener.local_addr()?.port());
        let out: JoinHandle<Result<TcpStream>> =
            spawn(async move { Ok(listener.accept().await?.0) });

        let IoConfig {
            hostname,
            js_socket_name,
            js_after_sockect_code,
            ..
        } = conf;

        let js_setup_code = format!(
            "
// Connect to the port and define socket
{js_socket_name} = require('net').connect('{shared_port}', '{hostname}');
///
;await ({js_after_sockect_code})({js_socket_name});
"
        );

        let _ = repl.run(&js_setup_code).await?;
        let mut stream = out.await.map_err(Error::RsSocketFail)??;

        let x = repl
            .run(
                "
console.log('24');
output('69');
",
            )
            .await?;
        println!("{}", String::from_utf8_lossy(&x));

        assert_eq!(x, b"24\n");
        let mut out = vec![0; 2];

        stream.read(&mut out).await?;
        assert_eq!(out, b"69");
        Ok(())
    }
}
