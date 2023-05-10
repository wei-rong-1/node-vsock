use napi::{self, Error};
use nix::errno::Errno;
// use log::error;

// pub trait ExitGracefully<T, E> {
//   fn ok_or_exit(self, message: &str) -> T;
// }

// impl<T, E: std::fmt::Debug> ExitGracefully<T, E> for Result<T, E> {
//   fn ok_or_exit(self, message: &str) -> T {
//     match self {
//       Ok(val) => val,
//       Err(err) => {
//         error!("{:?}: {}", err, message);
//         std::process::exit(1);
//       }
//     }
//   }
// }

pub(crate) fn error(msg: String) -> Error {
  Error::new(napi::Status::Unknown, msg)
}

pub(crate) fn nix_error(err: Errno) -> Error {
  error(format!("operation failed, errno: {}", err))
}

#[allow(dead_code)]
pub(crate) fn get_err() -> Error {
  let err = nix::errno::Errno::from_i32(nix::errno::errno());
  error(err.desc().to_string())
}
