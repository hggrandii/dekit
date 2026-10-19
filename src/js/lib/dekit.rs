use rquickjs::{Ctx, Exception, Object};

use crate::command::Command;
use crate::js::rquickjs_ext::ObjectExt;
use crate::protocol::{ActResult, RpcRequest};
use crate::runner::RunnerSpec;
use crate::target::Target;

pub(crate) struct RunnerStore(pub RunnerSpec);

unsafe impl<'js> rquickjs::JsLifetime<'js> for RunnerStore {
  type Changed<'to> = RunnerStore;
}

async fn run(ctx: &Ctx<'_>, command: Command) -> rquickjs::Result<usize> {
  let runner = ctx
    .userdata::<RunnerStore>()
    .ok_or_else(|| {
      Exception::throw_message(ctx, "runner identity is not initialized")
    })?
    .0
    .clone();
  let spawn = command.starts_runner();
  let value = crate::dekit::rpc_client::rpc_request(
    &runner,
    RpcRequest::Command(command),
    spawn,
  )
  .await
  .map_err(|err| Exception::throw_message(ctx, &err.to_string()))?;
  serde_json::from_value::<ActResult>(value)
    .map(|result| result.matched)
    .map_err(|err| Exception::throw_message(ctx, &err.to_string()))
}

fn target(ctx: &Ctx<'_>, value: String) -> rquickjs::Result<Target> {
  let target: Target =
    value.parse().map_err(|err: crate::target::InvalidTarget| {
      Exception::throw_message(ctx, &err.to_string())
    })?;
  if target.runner().is_some() {
    return Err(Exception::throw_message(
      ctx,
      "std.dekit targets are local to the script's runner",
    ));
  }
  Ok(target)
}

macro_rules! command_fn {
  ($name:ident, $variant:ident) => {
    async fn $name(ctx: Ctx<'_>, value: String) -> rquickjs::Result<usize> {
      let target = target(&ctx, value)?;
      run(&ctx, Command::$variant { target }).await
    }
  };
}

command_fn!(start, Start);
command_fn!(run_fresh, ForceRestart);
command_fn!(stop, Stop);
command_fn!(kill, Kill);
command_fn!(veto, Veto);
command_fn!(restart, Restart);
command_fn!(remove, Remove);

async fn up(ctx: Ctx<'_>) -> rquickjs::Result<usize> {
  run(&ctx, Command::Up).await
}

async fn add(
  ctx: Ctx<'_>,
  path: String,
  cmd: Vec<String>,
) -> rquickjs::Result<usize> {
  let target = target(&ctx, path)?;
  run(
    &ctx,
    Command::Add {
      target,
      spec: crate::spec::TaskSpec::with_cmd(cmd),
      place: Default::default(),
      start: true,
      defaults: true,
      origin: None,
    },
  )
  .await
}

pub fn init(ctx: Ctx<'_>) -> rquickjs::Result<Object<'_>> {
  let obj = Object::new(ctx)?;
  obj.def_fn_async("start", start)?;
  obj.def_fn_async("up", up)?;
  obj.def_fn_async("run", run_fresh)?;
  obj.def_fn_async("stop", stop)?;
  obj.def_fn_async("kill", kill)?;
  obj.def_fn_async("veto", veto)?;
  obj.def_fn_async("restart", restart)?;
  obj.def_fn_async("remove", remove)?;
  obj.def_fn_async("add", add)?;
  Ok(obj)
}
