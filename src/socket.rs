use crate::emitter::Emitter;
use crate::util::{error, nix_error};

use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, JsBuffer, JsFunction, JsNumber, JsString, JsUnknown, Result};
use nix::errno::Errno;
use nix::sys::socket::listen as listen_vsock;
use nix::sys::socket::{accept, bind, connect, recv, send, shutdown, socket};
use nix::sys::socket::{AddressFamily, MsgFlags, Shutdown, SockFlag, SockType, VsockAddr};
use nix::unistd::close as close_vsock;
use std::cmp::Ordering;
use std::convert::TryInto;
use std::os::unix::io::{AsRawFd, RawFd};
use std::thread;

// VSOCK defaut listen cid
const VMADDR_CID_ANY: u32 = 0xFFFFFFFF;

// Maximum number of buffer length
const BUF_MAX_LEN: usize = 8192;

// Maximum number of outstanding connections in the socket's listen queue
const BACKLOG: usize = 128;

// Maximum number of connection attempts
const MAX_CONNECTION_ATTEMPTS: usize = 5;

#[derive(Eq, Ord, PartialEq, PartialOrd, Copy, Clone)]
enum State {
  Initialized = 1,
  // ShuttingDown = 2,
  ShutDown = 3,
  Closed = 5,
}

#[napi]
pub struct VsockSocket {
  fd: RawFd,
  emitter: Emitter,
  state: State,
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
      .map_err(nix_error)?,
    };

    Ok(Self {
      fd,
      emitter: Emitter::new(env, emit_fn)?,
      state: State::Initialized,
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

    let server_fd = self.fd;
    let emit_error = self.thread_safe_emit_error()?;
    let emit_connection = self.thread_safe_emit_connection()?;

    thread::spawn(move || loop {
      println!("rs before accept");

      let fd = match accept(server_fd) {
        Ok(fd) => fd,
        Err(err) => {
          println!("rs accept err {0}", err);

          emit_error.call(
            Ok(format!("Accept connection failed: {0}", err)),
            ThreadsafeFunctionCallMode::Blocking,
          );
          continue;
        }
      };

      println!("rs accept emit");

      emit_connection.call(Ok(fd), ThreadsafeFunctionCallMode::Blocking);
    });

    Ok(())
  }

  #[napi]
  pub fn connect(&mut self, cid: JsNumber, port: JsNumber) -> Result<()> {
    let cid = cid.get_uint32()?;
    let port = port.get_uint32()?;
    let sockaddr = VsockAddr::new(cid, port);
    let fd = self.fd;
    let emit_error = self.thread_safe_emit_error()?;
    let emit_connect = self.thread_safe_emit_connect()?;

    thread::spawn(move || {
      let mut err_msg: Option<String> = None;
      for i in 0..MAX_CONNECTION_ATTEMPTS {
        match connect(fd, &sockaddr) {
          Ok(_) => {
            emit_connect.call(Ok(()), ThreadsafeFunctionCallMode::Blocking);
            break;
          }
          Err(err) => {
            err_msg = Some(format!("Connect socket failed: {}", err));
          }
        }
        // Exponentially backoff before retrying to connect to the socket
        std::thread::sleep(std::time::Duration::from_secs(1 << i));
      }

      if let Some(err_msg) = err_msg {
        emit_error.call(Ok(err_msg), ThreadsafeFunctionCallMode::Blocking);
      }
    });

    Ok(())
  }

  #[napi]
  pub fn end(&mut self) -> Result<()> {
    self.shutdown()?;
    Ok(())
  }

  #[napi]
  pub fn shutdown(&mut self) -> Result<()> {
    if self.state >= State::ShutDown {
      return Ok(());
    }

    shutdown(self.fd, Shutdown::Both).map_err(nix_error)?;
    self.state = State::ShutDown;
    self.emitter.emit_event("_shutdown")?;
    Ok(())
  }

  #[napi]
  pub fn close(&mut self) -> Result<()> {
    if self.state == State::Closed {
      return Ok(());
    }

    self.state = State::Closed;

    close_vsock(self.fd).map_err(nix_error)?;

    self.emitter.emit_event("close")?;
    self.emitter.unref()?;

    Ok(())
  }

  #[napi]
  pub fn start_recv(&mut self) -> Result<()> {
    if self.state > State::Initialized {
      return Err(error("can't start recv, bad state".to_string()));
    }

    self.hundle_recv()?;

    Ok(())
  }

  pub fn hundle_recv(&mut self) -> Result<()> {
    let fd = self.fd;
    let emit_error = self.thread_safe_emit_error()?;
    let emit_data = self.thread_safe_emit_data()?;
    let emit_end = self.thread_safe_emit_end()?;

    thread::spawn(move || loop {
      println!("rs before recv length");

      let mut buf: Vec<u8> = vec![0u8; BUF_MAX_LEN];
      let mut ret: i32 = -1;
      let mut ret_err = Errno::UnknownErrno;
      loop {
        match recv(fd, &mut buf, MsgFlags::empty()) {
          Ok(size) => {
            ret = size as i32;
          }
          Err(nix::Error::EINTR) => {
            continue;
          }
          Err(err) => {
            ret_err = err;
          }
        };
        break;
      }

      match ret.cmp(&0) {
        Ordering::Greater => {
          let size = ret as usize;
          let buf = buf[0..size].to_vec();
          println!("rs emit data {0}", String::from_utf8(buf.clone()).unwrap());
          emit_data.call(Ok(buf), ThreadsafeFunctionCallMode::Blocking);
        }
        Ordering::Less => {
          if ret_err == Errno::EAGAIN || ret_err == Errno::EWOULDBLOCK {
            // retry?
            continue;
          } else {
            emit_error.call(
              Ok(format!("read data failed: {0}", ret_err)),
              ThreadsafeFunctionCallMode::Blocking,
            );
            break;
          }
        }
        Ordering::Equal => {
          // when ret == 0
          emit_end.call(Ok(()), ThreadsafeFunctionCallMode::Blocking);
          break;
        }
      }
    });

    Ok(())
  }

  pub fn write(&mut self, buf: &[u8], len: u64) -> Result<()> {
    if self.state > State::Initialized {
      return Err(error("can't write, bad state".to_string()));
    }

    let len: usize = len.try_into().map_err(|err| error(format!("{:?}", err)))?;
    let mut send_bytes = 0;
    while send_bytes < len {
      let size = match send(self.fd, &buf[send_bytes..len], MsgFlags::empty()) {
        Ok(size) => size,
        Err(nix::Error::EINTR) => 0,
        Err(err) => return Err(error(format!("send data failed {:?}", err))),
      };
      send_bytes += size;
    }

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

  fn thread_safe_emit_connection(&mut self) -> Result<ThreadsafeFunction<i32>> {
    self.emitter.thread_safe_emit(|ctx| {
      // ctx.env.run_in_scope(|| {
      let fd = ctx.value;
      let mut args: Vec<JsUnknown> = vec![];
      let js_event = ctx.env.create_string("_connection")?;
      args.push(js_event.into_unknown());
      let js_fd = ctx.env.create_int32(fd)?;
      args.push(js_fd.into_unknown());
      Ok(args)
      // })
    })
  }

  fn thread_safe_emit_connect(&mut self) -> Result<ThreadsafeFunction<()>> {
    self.emitter.thread_safe_emit(|ctx| {
      // ctx.env.run_in_scope(|| {
      Ok(vec![ctx
        .env
        .create_string("_connect")
        .unwrap()
        .into_unknown()])
      // })
    })
  }

  fn thread_safe_emit_data(&mut self) -> Result<ThreadsafeFunction<Vec<u8>>> {
    self.emitter.thread_safe_emit(|ctx| {
      // ctx.env.run_in_scope(|| {
      let buf = ctx.value;
      let mut args: Vec<JsUnknown> = vec![];
      let js_event = ctx.env.create_string("_data")?;
      args.push(js_event.into_unknown());
      let js_buf = ctx.env.create_buffer_with_data(buf)?;
      args.push(js_buf.into_unknown());
      Ok(args)
      // })
    })
  }

  fn thread_safe_emit_end(&mut self) -> Result<ThreadsafeFunction<()>> {
    self.emitter.thread_safe_emit(|ctx| {
      // ctx.env.run_in_scope(|| {
      Ok(vec![ctx.env.create_string("end").unwrap().into_unknown()])
      // })
    })
  }

  fn thread_safe_emit_error(&mut self) -> Result<ThreadsafeFunction<String>> {
    self.emitter.thread_safe_emit(|ctx| {
      // ctx.env.run_in_scope(|| {
      let err = error(ctx.value);
      Ok(vec![
        ctx.env.create_string("_error").unwrap().into_unknown(),
        ctx.env.create_error(err).unwrap().into_unknown(),
      ])
      // })
    })
  }
}

impl Drop for VsockSocket {
  fn drop(&mut self) {
    self
      .shutdown()
      .unwrap_or_else(|err| eprintln!("Failed to shutdown socket: {:?}", err));

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
