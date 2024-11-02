#![allow(unused)]
use std::{net::SocketAddr, sync::Arc};

use crate::{Error, Repl, Result};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    spawn,
    sync::Mutex,
    task::JoinHandle,
};

/// "0" uses a random free port
pub const DEFAULT_PORT: &str = "0";
/// Default hostname used for rs js stream
pub const LOOPBACK: &str = "127.0.0.1";
/// Default name of the javascript variable that holds the stream
pub const DEFAULT_JS_SOCKET_NAME: &str = "socket";
/// Default code run after js socket
pub const DEFAULT_JS_AFTER_SOCKET_CODE: &str = "";

const DEFAULT_MILLIS: u64 = 100;

macro_rules! wait {
    ($millis:expr) => {
        tokio::time::sleep(Duration::from_millis($millis)).await;
    };
    () => {
        wait!(DEFAULT_MILLIS)
    };
}

macro_rules! pp {
    ($buf:expr) => {
        println!("{}", String::from_utf8_lossy(&$buf));
    };
}
/// Configuration for a Rust to Js stream
#[derive(derive_builder::Builder, Debug)]
#[builder(derive(Debug))]
pub struct RsJsStream {
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
    RsJsStream {
        hostname,
        js_socket_name,
        js_after_sockect_code,
        ..
    }: &RsJsStream,
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

type Streams = Arc<Mutex<Vec<Arc<Mutex<(TcpStream, SocketAddr)>>>>>;

/// create a stream connecting rust and js
pub async fn rust_js_stream(repl: &mut Repl, conf: &RsJsStream) -> Result<Streams> {
    let streams = Arc::new(Mutex::new(Vec::new()));
    let listener =
        TcpListener::bind(format!("{}:{}", conf.hostname, conf.rs_listener_port)).await?;
    let shared_port = format!("{}", listener.local_addr()?.port());
    let loop_streams = streams.clone();
    spawn(async move {
        loop {
            let (out, addr) = listener.accept().await?;
            loop_streams
                .lock()
                .await
                .push(Arc::new(Mutex::new((out, addr))));
        }
        Ok::<(), Error>(())
    });

    let js_setup_code = js_code(&shared_port, conf);
    let out = repl.run(&js_setup_code).await?;
    Ok(streams)
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::{sfb, Config};

    use super::*;
    #[tokio::test]
    async fn t_conf() -> Result<()> {
        // create first rs 2 js socket, make js side echo messages back through the socket
        let conf = RsJsStreamBuilder::default()
            .js_after_sockect_code(
                "(s) => {
                console.log('run socket before code');
                s.on('close', (...x) => {
                    console.log('close', ...x);
                });
                s.on('drain', (...x) => {
                    console.log('drain', ...x);
                });
                s.on('pipe', (...x) => {
                    console.log('pipe', ...x);
                });
                s.on('unpipe', (...x) => {
                    console.log('unpipe', ...x);
                });
                s.on('finish', (...x) => {
                    console.log('finish', ...x);
                });
                s.on('error', (...x) => {
                    console.log('error', ...x);
                });
                s.on('data', (chunk) => {
                    
                    s.write(`echo ${chunk.toString()}`);
                    console.log('after s.write');
                })
            }"
                .to_string(),
            )
            .build()?;

        let mut repl: Repl = Config::build()?.start().await?;
        let mut streams = rust_js_stream(&mut repl, &conf).await?;
        let mut out = vec![];
        loop {
            if { !streams.lock().await.is_empty() } {
                let len = { streams.lock().await.len() };
                dbg!(len);
                for i in 0..len {
                    dbg!(i);
                    let stream = { streams.lock().await[i].clone() };
                    dbg!(stream.lock().await.0.write(b"hola").await?);
                }
                break;
            }
        }
        loop {
            if { !streams.lock().await.is_empty() } {
                let len = { streams.lock().await.len() };
                dbg!(len);
                for i in 0..len {
                    let mut buf = vec![0; 1];
                    dbg!(i);
                    let stream = { streams.lock().await[i].clone() };
                    dbg!();
                    let mut s = &mut stream.lock().await.0;
                    dbg!(&s);
                    let x = s.read_exact(&mut buf).await?;
                    out.extend(buf);
                }
            }
            pp!(out);
        }
        assert_eq!(out, b"echo hola");

        Ok(())
    }
}
