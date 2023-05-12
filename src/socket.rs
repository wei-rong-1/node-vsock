use crate::emitter::Emitter;
use crate::util::{error, nix_error};

use byteorder::{ByteOrder, LittleEndian};
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, JsBuffer, JsFunction, JsNumber, JsString, JsUnknown, Result};
use nix::errno::Errno;
use nix::sys::socket::listen as listen_vsock;
use nix::sys::socket::{accept, bind, connect, recv, send, shutdown, socket};
use nix::sys::socket::{AddressFamily, MsgFlags, Shutdown, SockFlag, SockType, VsockAddr};
use nix::unistd::close as close_vsock;
use std::convert::TryInto;
use std::mem::size_of;
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
  // /**
  //  * Socket is marked to be shut down(write end).
  //  */
  // ShuttingDown = 2,
  // /**
  //  * Socket shut down(write end).
  //  */
  ShutDown = 3,
  // // Stopped = 4,
  // /**
  //  * Both read side and write side of the socket have been closed.
  //  */
  Closed = 5,
}

// fn send_u64(fd: RawFd, val: u64) -> Result<()> {
//   let mut buf = [0u8; size_of::<u64>()];
//   LittleEndian::write_u64(&mut buf, val);
//   send_loop(fd, &buf, size_of::<u64>().try_into().unwrap())?;
//   Ok(())
// }

// fn recv_u64(fd: RawFd) -> Result<u64> {
//   let mut buf = [0u8; size_of::<u64>()];
//   recv_loop(fd, &mut buf, size_of::<u64>().try_into().unwrap())?;
//   let val = LittleEndian::read_u64(&buf);
//   Ok(val)
// }

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

// fn recv_loop(fd: RawFd, buf: &mut [u8], len: u64) -> Result<()> {
//   let len: usize = len.try_into().map_err(|err| error(format!("{:?}", err)))?;
//   let mut recv_bytes = 0;

//   while recv_bytes < len {
//     let size = match recv(fd, &mut buf[recv_bytes..len], MsgFlags::empty()) {
//       Ok(size) => size,
//       Err(nix::Error::EINTR) => 0,
//       Err(err) => return Err(error(format!("recv data failed: {:?}", err))),
//     };
//     recv_bytes += size;
//   }

//   Ok(())
// }

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
      .map_err(|err| nix_error(err))?,
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

    let server_fd = self.fd.clone();
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
    let fd = self.fd.clone();
    let emit_error = self.thread_safe_emit_error()?;
    let emit_connect = self.thread_safe_emit_connect()?;

    thread::spawn(move || {
      let mut err_msg = String::new();
      for i in 0..MAX_CONNECTION_ATTEMPTS {
        match connect(fd, &sockaddr) {
          Ok(_) => {
            emit_connect.call(Ok(()), ThreadsafeFunctionCallMode::Blocking);
            break;
          }
          Err(e) => {
            err_msg = format!("Connect socket failed: {}", e);
          }
        }
        // Exponentially backoff before retrying to connect to the socket
        std::thread::sleep(std::time::Duration::from_secs(1 << i));
      }

      emit_error.call(Ok(err_msg), ThreadsafeFunctionCallMode::Blocking);
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
    shutdown(self.fd, Shutdown::Both).map_err(|err| nix_error(err))?;
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

    close_vsock(self.fd).map_err(|err| nix_error(err))?;

    self.emitter.emit_event("close")?;
    self.emitter.unref()?;

    Ok(())
  }

  #[napi]
  pub fn start_recv(&mut self) -> Result<()> {
    if self.state > State::Initialized {
      return Err(error(format!("can't start recv, bad state")));
    }

    self.hundle_recv()?;

    Ok(())
  }

  pub fn hundle_recv(&mut self) -> Result<()> {
    let fd = self.fd.clone();
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

      if ret > 0 {
        let size = ret as usize;
        let buf = buf[0..size].to_vec();
        println!("rs emit data {0}", String::from_utf8(buf.clone()).unwrap());
        emit_data.call(Ok(buf), ThreadsafeFunctionCallMode::Blocking);
      } else if ret < 0 {
        if ret_err == Errno::EAGAIN || ret_err == Errno::EWOULDBLOCK {
          // reset?
          // self.poll_events |= sys::uv_poll_event::UV_READABLE as i32;
          // self.reset_poll()?;
          // break;
        } else {
          emit_error.call(
            Ok(format!("read data failed: {0}", ret_err)),
            ThreadsafeFunctionCallMode::Blocking,
          );
          break;
        }
      } else {
        // if ret == 0
        emit_end.call(Ok(()), ThreadsafeFunctionCallMode::Blocking);
        // self.reset_poll()?;
        break;
      }
    });

    Ok(())
  }

  pub fn write(&mut self, buf: &[u8], len: u64) -> Result<()> {
    if self.state >= State::Initialized {
      return Err(error(format!("can't write, bad state")));
    }

    let fd = self.as_raw_fd();
    // send_u64(fd, len)
    //   .map_err(|err| error(format!("Failed to send buffer's length data {:?}", err)))?;
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
      // String::from_utf8(buf.to_vec()).map_err(|err| error(format!("The received bytes are not UTF-8: {:?}", err)))?
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
