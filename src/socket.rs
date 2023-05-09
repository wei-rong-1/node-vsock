use crate::protocol::{recv_loop, recv_u64, send_loop, send_u64};

use napi::{Env, JsBuffer, JsFunction, JsNumber, JsObject, JsString, JsUnknown, Ref, Result};
use nix::sys::socket::listen as listen_vsock;
use nix::sys::socket::{accept, bind, connect, shutdown, socket};
use nix::sys::socket::{AddressFamily, Shutdown, VsockAddr, SockFlag, SockType};
use nix::unistd::close;
use std::error::Error;
use std::convert::TryInto;
use std::os::unix::io::{AsRawFd, RawFd};

const VMADDR_CID_ANY: u32 = 0xFFFFFFFF;
const BUF_MAX_LEN: usize = 8192;
// Maximum number of outstanding connections in the socket's
// listen queue
const BACKLOG: usize = 128;
// Maximum number of connection attempts
const MAX_CONNECTION_ATTEMPTS: usize = 5;

#[derive(Debug, Clone)]
pub struct ServerArgs {
    pub port: u32,
}

#[derive(Debug, Clone)]
pub struct ClientArgs {
    pub cid: u32,
    pub port: u32,
}

#[napi]
struct VsockSocket {
  fd: RawFd,
}

#[napi]
impl VsockSocket {
  #[napi(constructor)]
  pub fn new() -> Result<VsockSocket> {
    let fd:RawFd = socket(
      AddressFamily::Vsock,
      SockType::Stream,
      SockFlag::empty(),
      None,
    );

    Ok(VsockSocket { fd })
  }

  pub fn write(&mut self, data: String) -> Result<()> {
    let fd = self.as_raw_fd();
    let buf = data.as_bytes();
    let len: u64 = buf.len().try_into().map_err(|err| format!("{:?}", err))?;
    send_u64(fd, len)?;
    send_loop(fd, buf, len)?;
  }

  pub fn listen(port: u32) -> Result<(), String> {
    let socket_fd = socket(
        AddressFamily::Vsock,
        SockType::Stream,
        SockFlag::empty(),
        None,
      )
      .map_err(|err| format!("Create socket failed: {:?}", err))?;
    
      let sockaddr = VsockAddr::new(VMADDR_CID_ANY, port);
    
      bind(socket_fd, &sockaddr).map_err(|err| format!("Bind failed: {:?}", err))?;
    
      listen_vsock(socket_fd, BACKLOG).map_err(|err| format!("Listen failed: {:?}", err))?;
    
      loop {
        let fd = accept(socket_fd).map_err(|err| format!("Accept failed: {:?}", err))?;
    
        // TODO: Replace this with your server code
        let len = recv_u64(fd)?;
        let mut buf = [0u8; BUF_MAX_LEN];
        recv_loop(fd, &mut buf, len)?;
        println!(
          "{}",
          String::from_utf8(buf.to_vec())
            .map_err(|err| format!("The received bytes are not UTF-8: {:?}", err))?
        );
      }
  }

//   pub fn connect()
}

impl Drop for VsockSocket {
  fn drop(&mut self) {
    shutdown(self.socket_fd, Shutdown::Both)
      .unwrap_or_else(|e| eprintln!("Failed to shut socket down: {:?}", e));
    close(self.socket_fd).unwrap_or_else(|e| eprintln!("Failed to close socket: {:?}", e));
  }
}

impl AsRawFd for VsockSocket {
  fn as_raw_fd(&self) -> RawFd {
    self.socket_fd
  }
}

/// Initiate a connection on an AF_VSOCK socket
fn vsock_connect(cid: u32, port: u32) -> Result<VsockSocket, String> {
  let sockaddr = VsockAddr::new(cid, port);
  let mut err_msg = String::new();

  for i in 0..MAX_CONNECTION_ATTEMPTS {
    let vsocket = VsockSocket::new(

    );

    match connect(vsocket.as_raw_fd(), &sockaddr) {
      Ok(_) => return Ok(vsocket),
      Err(e) => err_msg = format!("Failed to connect: {}", e),
    }

    // Exponentially backoff before retrying to connect to the socket
    std::thread::sleep(std::time::Duration::from_secs(1 << i));
  }

  Err(err_msg)
}

/// Send 'Hello, world!' to the server
pub fn client(args: ClientArgs) -> Result<(), String> {
  let vsocket = vsock_connect(args.cid, args.port)?;
  let fd = vsocket.as_raw_fd();

  // TODO: Replace this with your client code
  let data = "Hello, world!".to_string();
  let buf = data.as_bytes();
  let len: u64 = buf.len().try_into().map_err(|err| format!("{:?}", err))?;
  send_u64(fd, len)?;
  send_loop(fd, buf, len)?;

  Ok(())
}
