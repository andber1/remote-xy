//! RemoteXY library for Rust
//!
//! This library provides a simple way to connect your Rust application to the [RemoteXY](https://remotexy.com/) app.
//! You can use it to control and monitor your Rust application from your smartphone via a graphical interface you create yourself.
//! Connection is established via TCP/IP with your smartphone as the client.
//! The graphical interface is generated with the [RemoteXY online editor](https://remotexy.com/en/editor/).
//! You have to convert the generated C-structs into Rust-structs and provide the config buffer which is also generated on the website.
//!
//! Example:
//! ```rust
//! use remote_xy::remote_xy;
//! use remote_xy::RemoteXY;
//! use serde::Deserialize;
//! use serde::Serialize;
//!
//! const CONF_BUF: &[u8] = &[
//!     255, 2, 0, 2, 0, 59, 0, 16, 31, 1, 4, 0, 44, 10, 10, 78, 2, 26, 2, 0, 9, 77, 22, 11, 2, 26, 31,
//!     31, 79, 78, 0, 79, 70, 70, 0, 72, 12, 9, 16, 23, 23, 2, 26, 140, 38, 0, 0, 0, 0, 0, 0, 200, 66,
//!     0, 0, 0, 0, 70, 16, 16, 63, 9, 9, 26, 37, 0,
//! ];
//!
//! #[derive(Serialize, Deserialize, Default)]
//! #[repr(C)]
//! struct InputData {
//!     // input variables
//!     slider_1: i8, // =0..100 slider position
//!     switch_1: u8, // =1 if switch ON and =0 if OFF
//! }
//!
//! #[derive(Serialize, Deserialize, Default)]
//! #[repr(C)]
//! struct OutputData {
//!     // output variables
//!     circularbar_1: i8, // from 0 to 100
//!     led_1: u8,         // led state 0 .. 1
//!     // do not include the `connect_flag` variable
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // start server on port 6377
//!     let remotexy = remote_xy!(InputData, OutputData, "[::]:6377", CONF_BUF).await.unwrap();
//!     // Add an Ethernet device in the RemoteXY app
//!     // Do something with remotexy
//! }
//! ```

use anyhow::{bail, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch::{self, Receiver, Sender},
    task::JoinHandle,
};

const REMOTEXY_PACKAGE_START_BYTE: u8 = 0x55;
const HEADER_LEN: usize = 3;

/// RemoteXY struct that provides the interface to the RemoteXY app.
pub struct RemoteXY<I, O, const ISIZE: usize, const OSIZE: usize> {
    // We cannot use std::mem::size_of::<I>(). Thus additional const parameters
    // (ISIZE and OSIZE) are necessary. See https://github.com/rust-lang/rust/issues/43408
    input_rx: Receiver<[u8; ISIZE]>,
    output_tx: Sender<[u8; OSIZE]>,
    is_connected_rx: Receiver<bool>,
    server_handle: JoinHandle<()>,
    phantom: std::marker::PhantomData<(I, O)>,
}

/// This macro takes the InputData and OutputData types, the server address and the config buffer in order to
/// create a new RemoteXY struct.
/// The server is directly started.
#[macro_export]
macro_rules! remote_xy {
    ( $i:ty , $o:ty , $addr:expr, $conf:expr) => {{
        const ISIZE: usize = std::mem::size_of::<$i>();
        const OSIZE: usize = std::mem::size_of::<$o>();
        RemoteXY::<$i, $o, ISIZE, OSIZE>::new($addr, $conf)
    }};
}

impl<I, O, const ISIZE: usize, const OSIZE: usize> RemoteXY<I, O, ISIZE, OSIZE>
where
    I: Serialize + DeserializeOwned + Default,
    O: Serialize + DeserializeOwned + Default,
{
    /// Creates a new RemoteXY instance and starts the server. You have to provide the generic types
    /// InputData and OutputData as well as their sizes, e.g.:
    /// `RemoteXY::<InputData, OutputData, 2, 2>::new(LOCAL_ADDR, CONF_BUF).unwrap();`
    /// Prefer using the [`remote_xy!`](remote_xy) macro which calculates the correct sizes for you.
    pub async fn new(server_addr: &str, conf_buffer: &[u8]) -> Result<Self> {
        // parse config buffer and make some length checks
        let input_len;
        let output_len;
        let conf_len;
        let gui_config: Vec<u8>;
        if conf_buffer[0] == 0xff {
            input_len = conf_buffer[1] as usize | ((conf_buffer[2] as usize) << 8);
            output_len = conf_buffer[3] as usize | ((conf_buffer[4] as usize) << 8);
            conf_len = conf_buffer[5] as usize | ((conf_buffer[6] as usize) << 8);
            gui_config = conf_buffer[7..].to_vec();
        } else {
            input_len = conf_buffer[0] as usize;
            output_len = conf_buffer[1] as usize;
            conf_len = conf_buffer[2] as usize | ((conf_buffer[3] as usize) << 8);
            gui_config = conf_buffer[4..].to_vec();
        };
        if conf_len != gui_config.len() {
            bail!("Config buffer length mismatch");
        }
        if input_len != ISIZE {
            bail!("Input data length mismatch");
        }
        if output_len != OSIZE {
            bail!("Output data length mismatch");
        }

        // create channels to communicate with server
        let mut input_default = [0_u8; ISIZE];
        input_default.copy_from_slice(&bincode::serialize(&I::default())?);
        let mut output_default = [0_u8; OSIZE];
        output_default.copy_from_slice(&bincode::serialize(&O::default())?);
        let (input_tx, input_rx) = watch::channel(input_default);
        let (output_tx, output_rx) = watch::channel(output_default);
        let (is_connected_tx, is_connected_rx) = watch::channel(false);

        // start server
        let mut server = RemoteXYServer {
            server_addr: server_addr.into(),
            gui_config,
            input_tx,
            output_rx,
            is_connected_tx,
        };
        let server_handle =
            tokio::spawn(async move { server.run().await.expect("Unexpected error in server") });

        Ok(Self {
            input_rx,
            output_tx,
            is_connected_rx,
            server_handle,
            phantom: std::marker::PhantomData,
        })
    }

    /// Returns the current input data or the default value if no data is available.
    pub fn get_input(&self) -> I {
        bincode::deserialize(&*self.input_rx.borrow()).expect("Failed to deserialize input data")
    }

    /// Sets the output data. The data is sent to the RemoteXY app on your smartphone.
    pub fn set_output(&self, data: &O) {
        let mut buffer = [0_u8; OSIZE];
        buffer
            .copy_from_slice(&bincode::serialize(&data).expect("Failed to serialize output data"));
        self.output_tx
            .send(buffer)
            .expect("Failed to send output data");
    }

    /// Returns true if the RemoteXY app is connected to the server.
    pub fn is_connected(&self) -> bool {
        *self.is_connected_rx.borrow()
    }
}

impl<I, O, const ISIZE: usize, const OSIZE: usize> Drop for RemoteXY<I, O, ISIZE, OSIZE> {
    fn drop(&mut self) {
        log::info!("Closing server");
        self.server_handle.abort();
    }
}

struct RemoteXYServer<const ISIZE: usize, const OSIZE: usize> {
    server_addr: String,
    gui_config: Vec<u8>,
    input_tx: Sender<[u8; ISIZE]>,
    output_rx: Receiver<[u8; OSIZE]>,
    is_connected_tx: Sender<bool>,
}

impl<const ISIZE: usize, const OSIZE: usize> RemoteXYServer<ISIZE, OSIZE> {
    pub async fn run(&mut self) -> Result<()> {
        log::info!("Starting server at {}", &self.server_addr);
        let tcp_listener = TcpListener::bind(&self.server_addr).await?;

        loop {
            let (mut stream, _) = tcp_listener.accept().await?;
            log::debug!("New connection with {}", stream.peer_addr()?);
            self.is_connected_tx.send(true)?;

            loop {
                if self.handle_connection(&mut stream).await.is_err() {
                    self.is_connected_tx.send(false)?;
                    log::info!("Connection closed, listening for new connections...");
                    break;
                }
            }
        }
    }

    async fn handle_connection(&mut self, stream: &mut TcpStream) -> Result<()> {
        // read header with start byte and package length
        let mut header = [0; HEADER_LEN];
        stream.read_exact(&mut header).await?;
        if header[0] != REMOTEXY_PACKAGE_START_BYTE {
            bail!("Invalid package start byte");
        }
        let package_length = header[1] as u16 | ((header[2] as u16) << 8);

        // read body
        let mut body = vec![0; package_length as usize - HEADER_LEN];
        stream.read_exact(&mut body).await?;

        if !check_crc(&header, &body) {
            log::warn!("Invalid CRC, package is discarded");
            return Ok(());
        }
        let payload = &body[0..body.len() - 2];
        let response = self.parse_payload(payload).await;
        stream.write_all(&response).await?;
        stream.flush().await?;

        Ok(())
    }

    async fn parse_payload(&mut self, payload: &[u8]) -> Vec<u8> {
        let command = payload[0];
        match command {
            0x00 => self.gui_conf_response(),
            0x40 => self.data_response(),
            0x80 => {
                let mut buffer = [0_u8; ISIZE];
                buffer.copy_from_slice(&payload[1..]);
                self.input_tx.send(buffer).unwrap();

                self.ack_input()
            }
            0xC0 => self.output_data_response(),
            _ => {
                log::warn!("Received unknown command: 0x{command:X}");
                vec![]
            }
        }
    }

    fn data_response(&self) -> Vec<u8> {
        let mut input_and_output_buffer = Vec::from(*self.input_tx.borrow());
        input_and_output_buffer.extend_from_slice(&*self.output_rx.borrow());
        create_response(0x40, &input_and_output_buffer)
    }

    fn output_data_response(&self) -> Vec<u8> {
        let output_buffer = *self.output_rx.borrow();
        create_response(0xC0, &output_buffer)
    }

    fn ack_input(&self) -> Vec<u8> {
        create_response(0x80, &[])
    }

    fn gui_conf_response(&self) -> Vec<u8> {
        create_response(0x00, &self.gui_config)
    }
}

fn create_response(cmd: u8, buf: &[u8]) -> Vec<u8> {
    let package_length = buf.len() + 6;
    let mut send_buffer = vec![0; package_length];

    send_buffer[0] = REMOTEXY_PACKAGE_START_BYTE;
    send_buffer[1] = package_length as u8;
    send_buffer[2] = (package_length >> 8) as u8;
    send_buffer[3] = cmd;
    send_buffer[4..package_length - 2].copy_from_slice(buf);

    let crc = calc_crc(&send_buffer[0..package_length - 2]);
    send_buffer[package_length - 2] = crc as u8;
    send_buffer[package_length - 1] = (crc >> 8) as u8;
    send_buffer
}

fn check_crc(header: &[u8], body: &[u8]) -> bool {
    let crc = body[body.len() - 2] as u16 | ((body[body.len() - 1] as u16) << 8);
    let crc_calc = calc_crc(header);
    let crc_calc = calc_crc_with_start_value(&body[0..body.len() - 2], crc_calc);
    crc == crc_calc
}
fn calc_crc(buf: &[u8]) -> u16 {
    const REMOTEXY_INIT_CRC: u16 = 0xFFFF;
    calc_crc_with_start_value(buf, REMOTEXY_INIT_CRC)
}
fn calc_crc_with_start_value(buf: &[u8], start_val: u16) -> u16 {
    let mut crc: u16 = start_val;
    for b in buf {
        crc ^= *b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = ((crc) >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::timeout,
    };

    use crate::{check_crc, RemoteXY};
    use std::time::Duration;

    const CONF_BUF: &[u8] = &[
        255, 2, 0, 2, 0, 59, 0, 16, 31, 1, 4, 0, 44, 10, 10, 78, 2, 26, 2, 0, 9, 77, 22, 11, 2, 26,
        31, 31, 79, 78, 0, 79, 70, 70, 0, 72, 12, 9, 16, 23, 23, 2, 26, 140, 38, 0, 0, 0, 0, 0, 0,
        200, 66, 0, 0, 0, 0, 70, 16, 16, 63, 9, 9, 26, 37, 0,
    ];

    #[derive(Serialize, Deserialize, Default)]
    #[repr(C)]
    struct InputData {
        // input variables
        slider_1: i8, // =0..100 slider position
        switch_1: u8, // =1 if switch ON and =0 if OFF
    }

    #[derive(Serialize, Deserialize, Default)]
    #[repr(C)]
    struct OutputData {
        // output variables
        circularbar_1: i8, // from 0 to 100
        led_1: u8,         // led state 0 .. 1
    }

    #[tokio::test]
    async fn basic() {
        let remotexy = remote_xy!(InputData, OutputData, "127.0.0.1:6377", CONF_BUF)
            .await
            .unwrap();

        // test that client can start a connection
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut stream = TcpStream::connect("127.0.0.1:6377").await.unwrap();

        // ask for gui config
        stream.write_all(&[85, 6, 0, 0, 241, 233]).await.unwrap();
        stream.flush().await.unwrap();

        // check that gui config is returned (only check gui config itself which is part of config buffer, header or crc is not checked)
        let mut response_buffer = [0_u8; CONF_BUF.len() - 7 + 6];
        stream.read_exact(&mut response_buffer).await.unwrap();
        assert_eq!(response_buffer[4..response_buffer.len() - 2], CONF_BUF[7..]);

        // send some input data to server
        stream
            .write_all(&[85, 8, 0, 128, 57, 1, 63, 167])
            .await
            .unwrap();
        stream.flush().await.unwrap();

        // check that acknoledgement is sent
        let mut response_buffer = [0_u8; 6];
        stream.read_exact(&mut response_buffer).await.unwrap();
        assert_eq!(response_buffer, [85, 6, 0, 128, 240, 73]);

        // check that input data is received by server correctly
        let expected_input = InputData {
            slider_1: 57,
            switch_1: 1,
        };
        let actual_input = remotexy.get_input();
        assert_eq!(actual_input.slider_1, expected_input.slider_1);
        assert_eq!(actual_input.switch_1, expected_input.switch_1);

        // ask server for output data
        let output = OutputData {
            led_1: 1,
            circularbar_1: 57,
        };
        remotexy.set_output(&output);
        stream.write_all(&[85, 6, 0, 192, 241, 185]).await.unwrap();
        stream.flush().await.unwrap();

        // check response
        let mut response_buffer = [0_u8; 8];
        stream.read_exact(&mut response_buffer).await.unwrap();
        assert_eq!(response_buffer, [85, 8, 0, 192, 57, 1, 62, 115]);
    }

    #[tokio::test]
    async fn server_disconnect() {
        let remotexy = remote_xy!(InputData, OutputData, "127.0.0.1:6378", CONF_BUF)
            .await
            .unwrap();

        // client starts a connection
        tokio::time::sleep(Duration::from_millis(10)).await;
        let mut stream = TcpStream::connect("127.0.0.1:6378").await.unwrap();

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(remotexy.is_connected());

        // close server
        drop(remotexy);

        let mut buffer = [0_u8; 256];
        match timeout(Duration::from_secs(1), stream.read(&mut buffer)).await {
            Ok(Ok(size)) => assert_eq!(size, 0),
            Ok(Err(err)) => panic!("Server should have disconnected, error: {err}"),
            Err(err) => panic!("Server should have disconnected, error: {err}"),
        };
    }

    #[tokio::test]
    async fn client_disconnect() {
        let remotexy = remote_xy!(InputData, OutputData, "127.0.0.1:6379", CONF_BUF)
            .await
            .unwrap();

        // client starts a connection
        tokio::time::sleep(Duration::from_millis(10)).await;
        let stream = TcpStream::connect("127.0.0.1:6379").await.unwrap();

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(remotexy.is_connected());

        drop(stream);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!remotexy.is_connected());
    }

    #[test]
    fn crc() {
        // correct crc
        let header = [85, 6, 0];
        let body = [0, 241, 233];
        assert!(check_crc(&header, &body));

        let header = [85, 6, 0];
        let body = [192, 241, 185];
        assert!(check_crc(&header, &body));

        // wrong crc
        let body = [192, 242, 185];
        assert!(!check_crc(&header, &body));
    }
}
