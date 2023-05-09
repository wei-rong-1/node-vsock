use log::error;

pub trait ExitGracefully<T, E> {
  fn ok_or_exit(self, message: &str) -> T;
}

impl<T, E: std::fmt::Debug> ExitGracefully<T, E> for Result<T, E> {
  fn ok_or_exit(self, message: &str) -> T {
    match self {
      Ok(val) => val,
      Err(err) => {
        error!("{:?}: {}", err, message);
        std::process::exit(1);
      }
    }
  }
}

