use std::{fs, path, rc};

use asmbl_core as core;
use asmbl_utils as utils;

#[derive(Debug)]
struct ScriptError {
    underlying: rlua::Error,
    cause: Option<std::sync::Arc<ScriptError>>,
}

impl From<rlua::Error> for ScriptError {
    fn from(underlying: rlua::Error) -> Self {
        let cause = match underlying {
            rlua::Error::CallbackError { ref cause, .. } => Some(std::sync::Arc::new(
                ScriptError::from(rlua::Error::clone(cause)),
            )),
            _ => None,
        };
        Self { underlying, cause }
    }
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.underlying)
    }
}

impl failure::Fail for ScriptError {
    fn name(&self) -> Option<&str> {
        Some("ScriptError")
    }

    fn cause(&self) -> Option<&dyn failure::Fail> {
        use std::ops::Deref;
        self.cause
            .as_ref()
            .map(|b| -> &dyn failure::Fail { b.deref() })
    }
}

fn type_name(v: &rlua::Value) -> &'static str {
    match v {
        rlua::Value::Nil => "nil",
        rlua::Value::Boolean(_) => "boolean",
        rlua::Value::LightUserData(_) => "light userdata",
        rlua::Value::Integer(_) => "integer",
        rlua::Value::Number(_) => "number",
        rlua::Value::String(_) => "string",
        rlua::Value::Table(_) => "table",
        rlua::Value::Function(_) => "function",
        rlua::Value::Thread(_) => "thread",
        rlua::Value::UserData(_) | rlua::Value::Error(_) => "userdata",
    }
}

pub struct FrontEnd {
    lua: rlua::Lua,
}

impl FrontEnd {
    pub fn new() -> Self {
        Self {
            lua: rlua::Lua::new(),
        }
    }
}

#[derive(Clone)]
struct TaskSpecHandle {
    inner: core::TaskSpecHandle,
}

impl From<core::TaskSpecHandle> for TaskSpecHandle {
    fn from(inner: core::TaskSpecHandle) -> Self {
        Self { inner }
    }
}

impl Into<core::TaskSpecHandle> for TaskSpecHandle {
    fn into(self) -> core::TaskSpecHandle {
        self.inner
    }
}

struct PrerequisiteSpec {
    inner: core::PrerequisiteSpec,
}

impl PrerequisiteSpec {
    fn into_core(self) -> core::PrerequisiteSpec {
        self.inner
    }
}

impl<'lua> rlua::FromLua<'lua> for PrerequisiteSpec {
    fn from_lua(v: rlua::Value<'lua>, _: rlua::Context<'lua>) -> rlua::Result<Self> {
        match v {
            rlua::Value::String(s) => Ok(Self {
                inner: core::PrerequisiteSpec::Named(rc::Rc::from(path::Path::new(
                    s.to_str()?,
                ))),
            }),
            rlua::Value::UserData(u) => Ok(Self {
                inner: core::PrerequisiteSpec::Handle(
                    u.borrow::<TaskSpecHandle>()?.clone().into(),
                ),
            }),
            _ => Err(rlua::Error::FromLuaConversionError {
                from: type_name(&v),
                to: "PrerequisiteSpec",
                message: Some(String::from(
                    "Value must be the fully qualified name of a target \
                     or a handle returned from the task function",
                )),
            }),
        }
    }
}

impl rlua::UserData for TaskSpecHandle {}

fn make_lua_error<F: failure::Fail>(fail: F) -> rlua::Error {
    rlua::Error::external(failure::Error::from(fail))
}

enum SequenceIterator<'lua, T>
where
    T: rlua::FromLua<'lua>,
{
    Nil,
    Table(rlua::TableSequence<'lua, T>),
    Other(Option<(rlua::Context<'lua>, rlua::Value<'lua>)>),
}

impl<'lua, T> Iterator for SequenceIterator<'lua, T>
where
    T: rlua::FromLua<'lua>,
{
    type Item = Result<T, rlua::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Nil => None,
            Self::Table(ts) => ts.next(),
            Self::Other(o) => o.take().map(|(ctx, v)| T::from_lua(v, ctx)),
        }
    }
}

struct Sequence<'lua> {
    ctx: rlua::Context<'lua>,
    v: rlua::Value<'lua>,
}

impl<'lua> Sequence<'lua> {
    fn new(ctx: rlua::Context<'lua>, v: rlua::Value<'lua>) -> Self {
        Self { ctx, v }
    }

    fn into_iter<T>(self) -> SequenceIterator<'lua, T>
    where
        T: rlua::FromLua<'lua>,
    {
        match self.v {
            rlua::Value::Nil => SequenceIterator::Nil,
            rlua::Value::Table(t) => SequenceIterator::Table(t.sequence_values()),
            _ => SequenceIterator::Other(Some((self.ctx, self.v))),
        }
    }
}

impl core::FrontEnd for FrontEnd {
    fn parse_unit(&self, path: &path::Path) -> Result<core::Unit, failure::Error> {
        let script = utils::io::read_file(fs::File::open(path)?)?;

        self.lua.context(|ctx| {
            let mut unit = core::Unit::new();

            ctx.scope(|scope| -> Result<(), ScriptError> {
                ctx.globals().set(
                    "task",
                    scope.create_function_mut(
                        |ctx, args: rlua::Table| -> Result<TaskSpecHandle, _> {
                            let target: String = args.get("target")?;

                            let make_prequisite_specs = |key| -> Result<
                                Vec<core::PrerequisiteSpec>,
                                _,
                            > {
                                Sequence::new(ctx.clone(), args.get(key)?)
                                    .into_iter()
                                    .map(|r: Result<PrerequisiteSpec, _>| r.map(|p| p.into_core()))
                                    .collect()
                            };

                            let run = match args.get::<_, Option<rlua::Value>>("run")? {
                                Some(rlua::Value::Table(t)) => {
                                    core::Recipe::extract(t.sequence_values().collect::<Result<Vec<_>, _>>()?)
                                        .map_err(|err| make_lua_error(err))?
                                }
                                Some(rlua::Value::String(s)) => {
                                    core::Recipe::parse(s.to_str()?)
                                        .map_err(|err| make_lua_error(err))?
                                }
                                Some(v) => {
                                    return Err(rlua::Error::FromLuaConversionError {
                                        from: type_name(&v),
                                        to: "ExecRecipe",
                                        message: Some(String::from(
                                            "Value must be a string or a sequence of strings",
                                        )),
                                    });
                                }
                                None => {
                                    // FIXME - this will probably mean phony at some point in the future
                                    return Err(rlua::Error::FromLuaConversionError {
                                        from: "nil",
                                        to: "ExecRecipe",
                                        message: Some(String::from(
                                            "Value must be a string or a sequence of strings",
                                        )),
                                    });
                                }
                            };

                            Ok(unit
                                .add_task(
                                    target,
                                    make_prequisite_specs("consumes")?,
                                    make_prequisite_specs("depends_on")?,
                                    make_prequisite_specs("not_before")?,
                                    run,
                                )
                                .map_err(|err| make_lua_error(err))?
                                .into())
                        },
                    )?,
                )?;

                ctx.load(&script)
                    .set_name(path.to_str().unwrap_or("???"))?
                    .exec()?;

                Ok(())
            })?;

            Ok(unit)
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}