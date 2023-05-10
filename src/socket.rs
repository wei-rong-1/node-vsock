use crate::emitter::Emitter;
use crate::util::{error, nix_error};

use byteorder::{ByteOrder, LittleEndian};
use napi::{Env, JsBuffer, JsFunction, JsNumber, JsString, JsUnknown, Result};
use nix::sys::socket::listen as listen_vsock;
use nix::sys::socket::{accept, bind, connect, recv, send, shutdown, socket};
use nix::sys::socket::{AddressFamily, MsgFlags, Shutdown, SockFlag, SockType, VsockAddr};
use nix::unistd::close as close_vsock;
use std::convert::TryInto;
use std::mem::size_of;
use std::os::unix::io::{AsRawFd, RawFd};

// VSOCK defaut listen cid
const VMADDR_CID_ANY: u32 = 0xFFFFFFFF;

// Maximum number of buffer length
const BUF_MAX_LEN: usize = 8192;

// Maximum number of outstanding connections in the socket's listen queue
const BACKLOG: usize = 128;

// Maximum number of connection attempts
const MAX_CONNECTION_ATTEMPTS: usize = 5;

fn send_u64(fd: RawFd, val: u64) -> Result<()> {
  let mut buf = [0u8; size_of::<u64>()];
  LittleEndian::write_u64(&mut buf, val);
  send_loop(fd, &buf, size_of::<u64>().try_into().unwrap())?;
  Ok(())
}

fn recv_u64(fd: RawFd) -> Result<u64> {
  let mut buf = [0u8; size_of::<u64>()];
  recv_loop(fd, &mut buf, size_of::<u64>().try_into().unwrap())?;
  let val = LittleEndian::read_u64(&buf);
  Ok(val)
}

fn send_loop(fd: RawFd, buf: &[u8], len: u64) -> Result<()> {
  let len: usize = len.try_into().map_err(|err| error(format!("{:?}", err)))?;
  let mut send_bytes = 0;

  while send_bytes < len {
    let size = match send(fd, &buf[send_bytes..len], MsgFlags::empty()) {
      Ok(size) => size,
      Err(nix::Error::EINTR) => 0,
      Err(err) => return Err(error(format!("send data failed {:?}", err))),
    };
    send_bytes += size;
  }

  Ok(())
}

fn recv_loop(fd: RawFd, buf: &mut [u8], len: u64) -> Result<()> {
  let len: usize = len.try_into().map_err(|err| error(format!("{:?}", err)))?;
  let mut recv_bytes = 0;

  while recv_bytes < len {
    let size = match recv(fd, &mut buf[recv_bytes..len], MsgFlags::empty()) {
      Ok(size) => size,
      Err(nix::Error::EINTR) => 0,
      Err(err) => return Err(error(format!("recv data failed: {:?}", err))),
    };
    recv_bytes += size;
  }

  Ok(())
}

#[napi]
pub struct VsockSocket {
  fd: RawFd,
  env: Env,
  emitter: Emitter,
}

#[napi]
impl VsockSocket {
  #[napi(constructor)]
  pub fn new(env: Env, emit_fn: JsFunction, fd: Option<JsNumber>) -> Result<Self> {
    let fd: RawFd = match fd {
      Some(fd) => fd.get_int32()?,
      None => socket(
        AddressFamily::Vsock,
        SockType::Stream,
        SockFlag::empty(),
        None,
      )
      .map_err(|err| nix_error(err))?,
    };

    Ok(Self {
      fd,
      env,
      emitter: Emitter::new(env, emit_fn)?,
    })
  }

  #[napi]
  pub fn listen(&mut self, port: JsNumber) -> Result<()> {
    let server_fd = self.fd;
    let port = port.get_uint32()?;
    let sockaddr = VsockAddr::new(VMADDR_CID_ANY, port);

    bind(server_fd, &sockaddr)
      .map_err(|err| error(format!("Bind socket address failed: {:?}", err)))?;

    listen_vsock(server_fd, BACKLOG).map_err(|err| error(format!("Listen failed: {:?}", err)))?;

    loop {
      let env = self.env;
      let fd = match accept(self.fd) {
        Ok(fd) => fd,
        Err(err) => {
          self.emit_error(nix_error(err));
          continue;
        }
      };

      match env.run_in_scope(|| {
        let mut args: Vec<JsUnknown> = vec![];
        let js_event = env.create_string("_connection")?;
        args.push(js_event.into_unknown());
        let js_fd = env.create_int32(fd)?;
        args.push(js_fd.into_unknown());
        self.emitter.emit(&args)?;
        Ok(())
      }) {
        Ok(_) => {}
        Err(err) => {
          env
            .throw_error(&err.reason, None)
            .unwrap_or_else(|e| eprintln!("Emit _connection event failed: {:?}", e));
        }
      }
    }
  }

  #[napi]
  pub fn connect(&mut self, cid: JsNumber, port: JsNumber) -> Result<()> {
    let fd = self.fd;
    let cid = cid.get_uint32()?;
    let port = port.get_uint32()?;
    let sockaddr = VsockAddr::new(cid, port);

    let mut err_msg = String::new();

    for i in 0..MAX_CONNECTION_ATTEMPTS {
      match connect(fd, &sockaddr) {
        Ok(_) => {
          self.emitter.emit_event("_connect")?;
          return Ok(());
        }
        Err(e) => err_msg = format!("Connect socket failed: {}", e),
      }

      // Exponentially backoff before retrying to connect to the socket
      std::thread::sleep(std::time::Duration::from_secs(1 << i));
    }

    Err(error(err_msg))
  }

  #[napi]
  pub fn close(&mut self) -> Result<()> {
    // self.state = State::Closed;
    shutdown(self.fd, Shutdown::Both).map_err(|err| nix_error(err))?;
    close_vsock(self.fd).map_err(|err| nix_error(err))?;

    self.emitter.emit_event("close")?;
    self.emitter.unref()?;

    Ok(())
  }

  #[napi]
  pub fn start_recv(&mut self) -> Result<()> {
    loop {
      let env = self.env;
      let fd = self.fd;
      let len = match recv_u64(fd) {
        Ok(len) => len,
        Err(err) => {
          self.emit_error(err);
          continue;
        }
      };

      let mut buf = [0u8; BUF_MAX_LEN];
      match recv_loop(fd, &mut buf, len) {
        Ok(_) => {
          match env.run_in_scope(|| {
            let mut args: Vec<JsUnknown> = vec![];
            let js_event = env.create_string("_data")?;
            args.push(js_event.into_unknown());
            // buf[0..len].to_vec()
            // String::from_utf8(buf.to_vec()).map_err(|err| error(format!("The received bytes are not UTF-8: {:?}", err)))?
            let js_buf = env.create_buffer_with_data(buf.to_vec())?;
            args.push(js_buf.into_unknown());
            self.emitter.emit(&args)?;
            Ok(())
          }) {
            Ok(_) => {}
            Err(err) => {
              self.emit_error(err);
            }
          }
        }
        Err(err) => {
          self.emit_error(err);
        }
      }
    }
  }

  pub fn write(&mut self, buf: &[u8], len: u64) -> Result<()> {
    let fd = self.as_raw_fd();
    send_u64(fd, len)
      .map_err(|err| error(format!("Failed to send buffer's length data {:?}", err)))?;
    send_loop(fd, buf, len)
      .map_err(|err| error(format!("Failed to send buffer data {:?}", err)))?;

    Ok(())
  }

  #[napi]
  pub fn write_text(&mut self, data: JsString) -> Result<()> {
    let buf = data
      .into_utf8()
      .map_err(|err| error(format!("Get uft8 data failed {:?}", err)))?;
    let buf = buf.as_slice();
    let len: u64 = buf
      .len()
      .try_into()
      .map_err(|err| error(format!("Get utf8 data length failed {:?}", err)))?;

    self.write(buf, len)
  }

  #[napi]
  pub fn write_buffer(&mut self, data: JsBuffer) -> Result<()> {
    let data = data.into_value();
    let buf = data
      .as_ref()
      .map_err(|err| error(format!("Get buffer failed {:?}", err)))?;
    let len: u64 = buf
      .len()
      .try_into()
      .map_err(|err| error(format!("Get buffer length failed {:?}", err)))?;

    self.write(buf, len)
  }

  fn emit_error(&mut self, error: napi::Error) {
    let env = self.env;

    // TODO unwrap should be replaced with log
    env
      .run_in_scope(|| {
        let event = env.create_string("_error").unwrap();
        let error = self.env.create_error(error).unwrap();
        self
          .emitter
          .emit(&[event.into_unknown(), error.into_unknown()])
          .unwrap();
        Ok(())
      })
      .unwrap();
  }
}

impl Drop for VsockSocket {
  fn drop(&mut self) {
    self
      .close()
      .unwrap_or_else(|err| eprintln!("Failed to close socket: {:?}", err));
  }
}

impl AsRawFd for VsockSocket {
  fn as_raw_fd(&self) -> RawFd {
    self.fd
  }
}
