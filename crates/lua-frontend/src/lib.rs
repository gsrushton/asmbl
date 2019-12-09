use std::{fs, path};

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

impl Into<core::ParseUnitError> for ScriptError {
    fn into(self) -> core::ParseUnitError {
        core::ParseUnitError::Other(failure::Error::from(self))
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
struct TargetSpecHandle {
    inner: core::TargetSpecHandle,
}

impl From<core::TargetSpecHandle> for TargetSpecHandle {
    fn from(inner: core::TargetSpecHandle) -> Self {
        Self { inner }
    }
}

impl Into<core::TargetSpecHandle> for TargetSpecHandle {
    fn into(self) -> core::TargetSpecHandle {
        self.inner
    }
}

impl rlua::UserData for TargetSpecHandle {}

struct PrerequisiteSpec {
    inner: core::PrerequisiteSpec<path::PathBuf>,
}

impl Into<core::PrerequisiteSpec<path::PathBuf>> for PrerequisiteSpec {
    fn into(self) -> core::PrerequisiteSpec<path::PathBuf> {
        self.inner
    }
}

impl<'lua> rlua::FromLua<'lua> for PrerequisiteSpec {
    fn from_lua(v: rlua::Value<'lua>, _: rlua::Context<'lua>) -> rlua::Result<Self> {
        match v {
            rlua::Value::String(s) => Ok(Self {
                inner: core::PrerequisiteSpec::Named(path::PathBuf::from(s.to_str()?), false),
            }),
            rlua::Value::UserData(u) => Ok(Self {
                inner: core::PrerequisiteSpec::Handle(
                    u.borrow::<TargetSpecHandle>()?.clone().into(),
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

struct PathBuf {
    inner: path::PathBuf,
}

impl Into<path::PathBuf> for PathBuf {
    fn into(self) -> path::PathBuf {
        self.inner
    }
}

impl<'lua> rlua::FromLua<'lua> for PathBuf {
    fn from_lua(v: rlua::Value<'lua>, _: rlua::Context<'lua>) -> rlua::Result<Self> {
        match v {
            rlua::Value::String(s) => Ok(Self {
                inner: path::PathBuf::from(s.to_str()?),
            }),
            _ => Err(rlua::Error::FromLuaConversionError {
                from: type_name(&v),
                to: "Path",
                message: Some(String::from("Value must be a file path")),
            }),
        }
    }
}

struct TargetsSpec {
    inner: Vec<String>,
}

impl Into<Vec<String>> for TargetsSpec {
    fn into(self) -> Vec<String> {
        self.inner
    }
}

impl<'lua> rlua::FromLua<'lua> for TargetsSpec {
    fn from_lua(v: rlua::Value<'lua>, _: rlua::Context<'lua>) -> rlua::Result<Self> {
        match v {
            rlua::Value::String(s) => Ok(Self {
                inner: vec![s.to_str()?.to_string()],
            }),
            rlua::Value::Table(t) => Ok(Self {
                inner: t
                    .sequence_values::<String>()
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            _ => Err(rlua::Error::FromLuaConversionError {
                from: type_name(&v),
                to: "TargetsSpec",
                message: Some(String::from(
                    "Value must be the fully qualified name of a target \
                     or a handle returned from the task function",
                )),
            }),
        }
    }
}

struct TargetSpecHandleIterator {
    inner: core::TargetSpecHandleIterator,
}

impl From<core::TargetSpecHandleIterator> for TargetSpecHandleIterator {
    fn from(inner: core::TargetSpecHandleIterator) -> Self {
        Self { inner }
    }
}

impl<'lua> rlua::ToLuaMulti<'lua> for TargetSpecHandleIterator {
    fn to_lua_multi(self, ctx: rlua::Context<'lua>) -> Result<rlua::MultiValue<'lua>, rlua::Error> {
        use rlua::ToLua;
        Ok(self
            .inner
            .into_iter()
            .map(|handle| TargetSpecHandle::from(handle).to_lua(ctx.clone()))
            .collect::<Result<rlua::MultiValue<'lua>, _>>()?)
    }
}

impl core::FrontEnd for FrontEnd {
    fn parse_unit<'v, 'p>(
        &self,
        path: &path::Path,
        unit_builder: core::UnitBuilder<'v, 'p>,
    ) -> Result<core::Unit, core::ParseUnitError> {
        let script = utils::io::read_file(fs::File::open(path)?)?;

        self.lua.context(|ctx| {
            let unit_builder = std::cell::RefCell::new(unit_builder);

            ctx.scope(|scope| -> Result<(), ScriptError> {
                ctx.globals().set(
                    "task",
                    scope.create_function_mut(
                        |ctx, args: rlua::Table| -> Result<TargetSpecHandleIterator, _> {
                            let targets = match args.get::<_, Option<TargetsSpec>>("targets")? {
                                Some(targets) => targets,
                                None => args.get("target")?,
                            };

                            let make_prequisite_specs =
                                |key| -> Result<Vec<core::PrerequisiteSpec<path::PathBuf>>, _> {
                                    Sequence::new(ctx.clone(), args.get(key)?)
                                        .into_iter()
                                        .map(|r: Result<PrerequisiteSpec, _>| r.map(|p| p.into()))
                                        .collect()
                                };

                            let run = match args.get::<_, Option<rlua::Value>>("run")? {
                                Some(rlua::Value::Table(t)) => core::Recipe::new(
                                    t.sequence_values().collect::<Result<Vec<_>, _>>()?,
                                )
                                .map_err(|err| make_lua_error(err))?,
                                Some(rlua::Value::String(s)) => core::Recipe::parse(s.to_str()?)
                                    .map_err(|err| make_lua_error(err))?,
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

                            let env: Vec<core::EnvSpec> = match args
                                .get::<_, Option<rlua::Table>>("env")?
                            {
                                Some(t) => t
                                    .pairs::<rlua::Value, String>()
                                    .map(|pair| {
                                        let (name, value) = pair?;
                                        match name {
                                            rlua::Value::Number(_) | rlua::Value::Integer(_) => {
                                                Ok(core::EnvSpec::inherit(value))
                                            }
                                            rlua::Value::String(s) => Ok(core::EnvSpec::define(
                                                s.to_str()?.to_string(),
                                                value,
                                            )),
                                            _ => Err(rlua::Error::FromLuaConversionError {
                                                from: type_name(&name),
                                                to: "EnvSpecKey",
                                                message: Some(String::from(
                                                    "Value must be a string or number",
                                                )),
                                            }),
                                        }
                                    })
                                    .collect::<Result<Vec<_>, _>>()?,
                                None => vec![],
                            };

                            Ok(unit_builder
                                .borrow_mut()
                                .add_task(
                                    targets.into(),
                                    make_prequisite_specs("consumes")?,
                                    make_prequisite_specs("depends_on")?,
                                    make_prequisite_specs("not_before")?,
                                    env,
                                    run,
                                )
                                .map_err(|err| make_lua_error(err))?
                                .into())
                        },
                    )?,
                )?;

                ctx.globals().set(
                    "sub_unit",
                    scope.create_function_mut(|_, sub_unit: PathBuf| -> Result<(), _> {
                        unit_builder
                            .borrow_mut()
                            .add_sub_unit(sub_unit.into())
                            .map_err(|err| make_lua_error(err))?;
                        Ok(())
                    })?,
                )?;

                ctx.globals().set(
                    "include",
                    scope.create_function_mut(|_, target: TargetSpecHandle| -> Result<(), _> {
                        unit_builder.borrow_mut().add_include(target.into());
                        Ok(())
                    })?,
                )?;

                ctx.load(&script)
                    .set_name(path.to_string_lossy().as_ref())?
                    .exec()?;

                Ok(())
            })
            .map_err(|err| -> core::ParseUnitError { err.into() })?;

            Ok(unit_builder.into_inner().unit())
        })
    }
}
