use std::{ffi, path, rc};

use crate::env::{EnvSpec, EnvSpecValue};
use crate::targets::Targets;

mod parser;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Variable {
    Targets,
    Target(usize),
    Inputs,
    Input(usize),
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgElement {
    Str(String),
    Var(Variable),
    Break,
}

#[derive(Debug, failure::Fail)]
pub enum RecipePrepareError {
    #[fail(display = "Unable to convert path to unicode.")]
    NonUnicodePath,
    #[fail(display = "Recipe string must contain at least the command to run.")]
    NotEnoughArgs,
    #[fail(display = "Couldn't find recipe command '{}'.", 0)]
    NoSuchCmd(String),
    #[fail(display = "Input index '{}' is out-of-range.", 0)]
    InputIndexOutOfRange(usize),
    #[fail(display = "Target index '{}' is out-of-range.", 0)]
    TargetIndexOutOfRange(usize),
    #[fail(display = "Unrecognised bindings '{}'.", 0)]
    UnrecognisedBinding(String),
}

#[derive(Debug, failure::Fail)]
pub enum RecipeParseError {
    #[fail(display = "Failed to parse individual arguments from string.")]
    ParseArgError,
    #[fail(display = "Failed to parse elements from an individual argument.")]
    ParseElementError,
    #[fail(display = "Recipe string must contain at least the command to run.")]
    NotEnoughArgs,
}

impl From<parser::ParseArgsError> for RecipeParseError {
    fn from(_: parser::ParseArgsError) -> Self {
        Self::ParseArgError
    }
}

impl From<parser::ParseElementsError> for RecipeParseError {
    fn from(_: parser::ParseElementsError) -> Self {
        Self::ParseElementError
    }
}

#[derive(Clone, Debug)]
pub struct Recipe {
    elements: Vec<ArgElement>,
}

impl Recipe {
    pub fn new(args: Vec<String>) -> Result<Self, RecipeParseError> {
        if args.len() == 0 {
            Err(RecipeParseError::NotEnoughArgs)
        } else {
            let mut elements = vec![];
            for arg in args.into_iter() {
                elements.extend(parser::parse_elements(&arg)?);
                elements.push(ArgElement::Break);
            }
            Ok(Self { elements })
        }
    }

    pub fn parse(s: &str) -> Result<Self, RecipeParseError> {
        Self::new(parser::parse_args(s)?)
    }

    pub fn prepare(
        &self,
        // Wouldn't it be nice if these were all moves...
        targets: &Targets,
        inputs: &Vec<rc::Rc<path::Path>>,
        env: &Vec<EnvSpec>,
    ) -> Result<std::process::Command, RecipePrepareError> {
        let targets = targets
            .iter()
            .map(|path| path.to_str().ok_or(RecipePrepareError::NonUnicodePath))
            .collect::<Result<Vec<_>, RecipePrepareError>>()?;

        let inputs = inputs
            .iter()
            .map(|input| input.to_str().ok_or(RecipePrepareError::NonUnicodePath))
            .collect::<Result<Vec<_>, RecipePrepareError>>()?;

        let mut args = vec![];

        let mut e = 0;
        while e < self.elements.len() {
            let mut arg = String::with_capacity(32);
            while e < self.elements.len() && self.elements[e] != ArgElement::Break {
                match &self.elements[e] {
                    ArgElement::Str(s) => arg.push_str(&s),
                    ArgElement::Var(v) => match v {
                        Variable::Input(index) => {
                            if *index >= inputs.len() {
                                return Err(RecipePrepareError::InputIndexOutOfRange(*index));
                            }
                            arg.push_str(inputs[*index])
                        }
                        Variable::Target(index) => {
                            if *index >= targets.len() {
                                return Err(RecipePrepareError::TargetIndexOutOfRange(*index));
                            }
                            arg.push_str(targets[*index])
                        }
                        Variable::Inputs => arg.push_str(&inputs.join(" ")),
                        Variable::Targets => arg.push_str(&targets.join(" ")),
                        Variable::Other(name) => {
                            return Err(RecipePrepareError::UnrecognisedBinding(name.to_owned()))
                        }
                    },
                    ArgElement::Break => unreachable!(),
                }
                e += 1;
            }
            args.push(arg);
            e += 1;
        }

        let (cmd, args) = args
            .split_first()
            .ok_or(RecipePrepareError::NotEnoughArgs)?;

        let cmd_path = path::PathBuf::from(cmd);
        let cmd_path = if cmd_path.exists() {
            Some(cmd_path)
        } else {
            match std::env::var_os("PATH") {
                Some(paths) => std::env::split_paths(&paths)
                    .map(|path| path.join(cmd))
                    .find(|path| path.exists()),
                None => None,
            }
        }
        .ok_or_else(|| RecipePrepareError::NoSuchCmd(cmd.to_owned()))?;

        let mut cmd = std::process::Command::new(&cmd_path);
        cmd.args(args)
            .env_clear()
            .envs(env.into_iter().filter_map(|env| {
                let value = match env.value() {
                    EnvSpecValue::INHERIT => std::env::var_os(&env.name()),
                    EnvSpecValue::DEFINE(value) => Some(ffi::OsString::from(value)),
                };
                value.map(|v| (env.name().clone(), v))
            }));
        Ok(cmd)
    }
}
