use std::{ffi, path, rc};

use crate::env::{EnvSpec, EnvSpecValue};
use crate::targets::Targets;

#[derive(Debug, failure::Fail)]
pub enum RecipePrepareError {
  #[fail(display = "Unable to convert path to unicode.")]
  NonUnicodePath
}

#[derive(Debug, failure::Fail)]
pub enum RecipeParseError {
    #[fail(display = "Recipe string contains mis-matched quotes")]
    MismatchedQuotes,
    #[fail(display = "Recipe string must contain at least the command to run")]
    NotEnoughArgs,
    #[fail(display = "Couldn't find recipe command '{}'", 0)]
    NoSuchCmd(String),
}

impl From<shellwords::MismatchedQuotes> for RecipeParseError {
    fn from(_: shellwords::MismatchedQuotes) -> Self {
        Self::MismatchedQuotes
    }
}

#[derive(Debug)]
pub struct Recipe {
    cmd_path: path::PathBuf,
    args: Vec<String>,
}

impl Recipe {
    pub fn new(cmd: &str, args: Vec<String>) -> Result<Self, RecipeParseError> {
        let cmd_path = path::Path::new(cmd);

        let cmd_path = if cmd_path.exists() {
            Some(cmd_path.to_path_buf())
        } else {
            match std::env::var_os("PATH") {
                Some(paths) => std::env::split_paths(&paths)
                    .map(|path| path.join(cmd))
                    .find(|path| path.exists()),
                None => None,
            }
        }
        .ok_or_else(|| RecipeParseError::NoSuchCmd(cmd.to_owned()))?;

        Ok(Self { cmd_path, args })
    }

    pub fn extract(args: Vec<String>) -> Result<Self, RecipeParseError> {
        if let Some((cmd, args)) = args.split_first() {
            // Think this can be done without so much cloning
            Self::new(cmd, args.to_vec())
        } else {
            Err(RecipeParseError::NotEnoughArgs)
        }
    }

    pub fn parse(s: &str) -> Result<Self, RecipeParseError> {
        Self::extract(shellwords::split(s)?)
    }

    pub fn prepare(
        &self,
        // Wouldn't it be nice if these were all moves...
        targets: &Targets,
        inputs: &Vec<rc::Rc<path::Path>>,
        env: &Vec<EnvSpec>,
    ) -> Result<std::process::Command, RecipePrepareError> {
        use regex::{Captures, Regex};

        lazy_static! {
            static ref RE: Regex = Regex::new(r"\$\{(\w+)\}").unwrap();
        }

        let target = targets
            .iter()
            .map(|path| path.to_str().ok_or(RecipePrepareError::NonUnicodePath))
            .collect::<Result<Vec<_>, RecipePrepareError>>()?
            .join(" ");

        let inputs = inputs
            .iter()
            .map(|input| input.to_str().ok_or(RecipePrepareError::NonUnicodePath))
            .collect::<Result<Vec<_>, RecipePrepareError>>()?
            .join(" ");

        let mut cmd = std::process::Command::new(&self.cmd_path);
        cmd.args(self.args.iter().map(|arg| {
            RE.replace_all(&arg, |caps: &Captures| match caps[1].as_ref() {
                "target" => target.clone(),
                "inputs" => inputs.clone(),
                _ => caps[1].to_owned(),
            })
            .into_owned()
        }))
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
