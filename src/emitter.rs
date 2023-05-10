use crate::util::error;
use napi::{Env, JsFunction, JsUnknown, Ref, Result};

pub(crate) struct Emitter {
  env: Env,
  emit_ref: Option<Ref<()>>,
}

impl Drop for Emitter {
  fn drop(&mut self) {
    self.unref().unwrap();
  }
}

impl Emitter {
  pub fn new(env: Env, emit: JsFunction) -> Result<Self> {
    let emit_ref = env.create_reference(emit)?;

    Ok(Self {
      env,
      emit_ref: Some(emit_ref),
    })
  }

  pub fn unref(&mut self) -> Result<()> {
    let mut emit_ref = self.emit_ref.take();

    match emit_ref.as_mut() {
      None => (),
      Some(emit_ref) => {
        emit_ref.unref(self.env)?;
      }
    }

    Ok(())
  }

  fn check_ref(&self) -> Result<()> {
    if self.emit_ref.is_none() {
      return Err(error("emitter already unreferenced".to_string()));
    }

    Ok(())
  }

  pub fn emit(&mut self, args: &[JsUnknown]) -> Result<()> {
    self.check_ref()?;

    let env = self.env;

    env.run_in_scope(|| {
      let emit_ref = self.emit_ref.as_mut().unwrap();
      let emit: JsFunction = env.get_reference_value(emit_ref)?;
      emit.call(None, args)?;
      Ok(())
    })?;

    Ok(())
  }

  pub fn emit_event(&mut self, event: &str) -> Result<()> {
    let env = self.env;
    env.run_in_scope(|| {
      let js_event = env.create_string(event)?;
      let mut args: Vec<JsUnknown> = vec![];
      args.push(js_event.into_unknown());

      self.emit(&args)
    })?;
    Ok(())
  }
}
