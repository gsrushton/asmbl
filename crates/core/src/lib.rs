#[macro_use]
extern crate lazy_static;

use std::{collections, ffi, fs, path, rc, time::SystemTime};

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
        target: &path::Path,
        inputs: &Vec<rc::Rc<path::Path>>,
        // Wouldn't it be nice if this was a move...
        env: &Vec<EnvSpec>,
    ) -> Result<std::process::Command, CakeError> {
        use regex::{Captures, Regex};

        lazy_static! {
            static ref RE: Regex = Regex::new(r"\$\{(\w+)\}").unwrap();
        }

        let target = target.to_str().ok_or(CakeError::NonUnicodePath)?;

        let inputs = inputs
            .iter()
            .map(|input| input.to_str().ok_or(CakeError::NonUnicodePath))
            .collect::<Result<Vec<_>, CakeError>>()?
            .join(" ");

        let mut cmd = std::process::Command::new(&self.cmd_path);
        cmd.args(self.args.iter().map(|arg| {
            RE.replace_all(&arg, |caps: &Captures| match caps[1].as_ref() {
                "target" => target.to_owned(),
                "inputs" => inputs.clone(),
                _ => caps[1].to_owned(),
            })
            .into_owned()
        }))
        .env_clear()
        .envs(env.into_iter().filter_map(|env| {
            let value = match &env.value {
                EnvSpecValue::INHERIT => std::env::var_os(&env.name),
                EnvSpecValue::DEFINE(value) => Some(ffi::OsString::from(value)),
            };
            value.map(|v| (env.name.clone(), v))
        }));
        Ok(cmd)
    }
}

#[derive(Debug)]
enum Prerequisite {
    Named(rc::Rc<path::Path>),
    Handle(TaskHandle),
}

pub enum PrerequisiteSpec {
    Named(rc::Rc<path::Path>),
    Handle(TaskSpecHandle),
}

#[derive(Debug)]
pub enum EnvSpecValue {
    INHERIT,
    DEFINE(String),
}

#[derive(Debug)]
pub struct EnvSpec {
    name: String,
    value: EnvSpecValue,
}

impl EnvSpec {
    pub fn define(name: String, value: String) -> Self {
        Self {
            name,
            value: EnvSpecValue::DEFINE(value),
        }
    }

    pub fn inherit(name: String) -> Self {
        Self {
            name,
            value: EnvSpecValue::INHERIT,
        }
    }
}

pub struct TaskSpec {
    consumes: Vec<PrerequisiteSpec>,
    depends_on: Vec<PrerequisiteSpec>,
    not_before: Vec<PrerequisiteSpec>,
    env: Vec<EnvSpec>,
    recipe: Recipe,
}

impl TaskSpec {
    fn new(
        consumes: Vec<PrerequisiteSpec>,
        depends_on: Vec<PrerequisiteSpec>,
        not_before: Vec<PrerequisiteSpec>,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> Self {
        Self {
            consumes,
            depends_on,
            not_before,
            env,
            recipe: recipe,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskHandle {
    index: usize,
}

impl TaskHandle {
    fn new(index: usize) -> Self {
        Self { index }
    }
}

#[derive(Clone, Copy)]
pub struct TaskSpecHandle {
    index: usize,
}

impl TaskSpecHandle {
    fn new(index: usize) -> Self {
        Self { index }
    }

    fn resolve(self, offset: usize) -> TaskHandle {
        TaskHandle::new(self.index + offset)
    }
}

pub struct Unit {
    tasks: Vec<(path::PathBuf, TaskSpec)>,
    prerequisites: Vec<(path::PathBuf, path::PathBuf)>,
    sub_units: Vec<(path::PathBuf, bool)>,
}

impl Unit {
    fn new() -> Self {
        Self {
            tasks: vec![],
            prerequisites: vec![],
            sub_units: vec![],
        }
    }

    pub fn add_task(
        &mut self,
        target: path::PathBuf,
        consumes: Vec<PrerequisiteSpec>,
        depends_on: Vec<PrerequisiteSpec>,
        not_before: Vec<PrerequisiteSpec>,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> TaskSpecHandle {
        let handle = TaskSpecHandle::new(self.tasks.len());
        self.tasks.push((
            target,
            TaskSpec::new(consumes, depends_on, not_before, env, recipe),
        ));
        handle
    }

    pub fn add_prerequisite(&mut self, target: path::PathBuf, prerequisite: path::PathBuf) {
        self.prerequisites.push((target, prerequisite))
    }

    pub fn add_sub_unit(&mut self, file: path::PathBuf, optional: bool) {
        self.sub_units.push((file, optional))
    }

    pub fn decompose(
        self,
    ) -> (
        Vec<(path::PathBuf, TaskSpec)>,
        Vec<(path::PathBuf, path::PathBuf)>,
    ) {
        (self.tasks, self.prerequisites)
    }
}

#[derive(Debug, failure::Fail)]
pub enum RelativiseError {
    #[fail(display = "File path addresses beneath root.")]
    Underflow,
    #[fail(display = "Path prefixes are unsupported.")]
    PrefixUnsupported,
}

pub struct UnitBuilder<'p, 'v> {
    context: &'v Vec<path::Component<'p>>,
    base: path::PathBuf,
    unit: Unit,
}

impl<'p, 'v> UnitBuilder<'p, 'v> {
    pub fn new(context: &'v Vec<path::Component<'p>>, base: path::PathBuf) -> Self {
        Self {
            context,
            base,
            unit: Unit::new(),
        }
    }

    pub fn add_task(
        &mut self,
        target: &path::Path,
        consumes: Vec<PrerequisiteSpec>,
        depends_on: Vec<PrerequisiteSpec>,
        not_before: Vec<PrerequisiteSpec>,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> Result<TaskSpecHandle, RelativiseError> {
        let target = self.relativise(&target)?;

        let relativise_prequisite = |prerequisite| match prerequisite {
            PrerequisiteSpec::Named(name) => Ok(PrerequisiteSpec::Named(rc::Rc::from(
                self.relativise(&name)?,
            )
                as rc::Rc<path::Path>)),
            _ => Ok(prerequisite),
        };

        let consumes = consumes
            .into_iter()
            .map(relativise_prequisite)
            .collect::<Result<Vec<_>, _>>()?;

        let depends_on = depends_on
            .into_iter()
            .map(relativise_prequisite)
            .collect::<Result<Vec<_>, _>>()?;

        let not_before = not_before
            .into_iter()
            .map(relativise_prequisite)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(self
            .unit
            .add_task(target, consumes, depends_on, not_before, env, recipe))
    }

    pub fn add_prerequisite(
        &mut self,
        target: &path::Path,
        prerequisite: &path::Path,
    ) -> Result<(), RelativiseError> {
        Ok(self
            .unit
            .add_prerequisite(self.relativise(&target)?, self.relativise(&prerequisite)?))
    }

    pub fn add_sub_unit(
        &mut self,
        file: &path::Path,
        optional: bool,
    ) -> Result<(), RelativiseError> {
        Ok(self.unit.add_sub_unit(self.relativise(&file)?, optional))
    }

    pub fn unit(self) -> Unit {
        self.unit
    }

    fn relativise(&self, path: &path::Path) -> Result<path::PathBuf, RelativiseError> {
        use std::borrow::Cow;

        let abs = if path.is_absolute() {
            Cow::Borrowed(path)
        } else {
            Cow::Owned(self.base.join(path))
        };

        let mut components: Vec<path::Component> = Vec::new();
        for component in abs.components() {
            match component {
                path::Component::CurDir => { /*NOP*/ }
                path::Component::ParentDir => {
                    if let None = components.pop() {
                        return Err(RelativiseError::Underflow);
                    }
                }
                _ => components.push(component),
            }
        }

        let mut shared_component_index: usize = 0;
        while shared_component_index < std::cmp::min(components.len(), self.context.len())
            && components[shared_component_index] == self.context[shared_component_index]
        {
            shared_component_index += 1;
        }

        let mut path = path::PathBuf::new();

        // Walk backwards until we match with context..
        for _ in shared_component_index..self.context.len() {
            path.push("..");
        }

        // Walk forwards building the rest of the path...
        for c in shared_component_index..components.len() {
            path.push(components[c]);
        }

        Ok(path)
    }
}

#[derive(Debug, failure::Fail)]
pub enum CakeError {
    #[fail(display = "I/O error {:?}.", 0)]
    IoError(path::PathBuf, #[fail(cause)] std::io::Error),
    #[fail(display = "Prerequisite {:?} unavailable.", 0)]
    PrerequisiteMissing(path::PathBuf, #[fail(cause)] std::io::Error),
    #[fail(
        display = "Unable to determine the last modification time of the prerequisite {:?}.",
        0
    )]
    NoLastModifiedTime(path::PathBuf, #[fail(cause)] std::io::Error),
    #[fail(display = "Unable to convert path to unicode.")]
    NonUnicodePath,
}

#[derive(Debug)]
pub struct Task {
    target: rc::Rc<path::Path>,
    inputs: Vec<rc::Rc<path::Path>>,
    upstream: Vec<Prerequisite>,
    downstream: Vec<TaskHandle>,
    env: Vec<EnvSpec>,
    recipe: Recipe,
}

impl Task {
    // TODO wouldn't it be nice if the was self
    pub fn prepare(&self) -> Result<std::process::Command, CakeError> {
        self.recipe.prepare(&self.target, &self.inputs, &self.env)
    }
}

#[derive(Debug)]
pub struct TaskList {
    tasks: Vec<Task>,
}

impl TaskList {
    pub fn new(units: Vec<Unit>) -> Self {
        // Extract the list of tasks from each unit,
        // flattening them into one big list.
        let (specs, prerequisites): (Vec<_>, Vec<_>) =
            units.into_iter().map(|unit| unit.decompose()).unzip();

        let (targets, mut specs): (Vec<_>, Vec<_>) = specs
            .into_iter()
            .scan(0, |count, it| {
                let offset = *count;
                *count += it.len();
                Some(it.into_iter().map(move |(target, spec)| {
                    (rc::Rc::from(target) as rc::Rc<path::Path>, (spec, offset))
                }))
            })
            .flatten()
            .unzip();

        // Build a flat list of files and a map from
        // file-path to index within that list.
        let target_lut: collections::HashMap<_, _> = targets
            .iter()
            .enumerate()
            .map(|(index, target)| (target, index))
            .collect();

        // Account for any extra prerequisites.
        for (target, prerequisite) in prerequisites.into_iter().flatten() {
            let target = rc::Rc::from(target);
            match target_lut.get(&target) {
                Some(index) => {
                    specs[*index]
                        .0
                        .depends_on
                        .push(PrerequisiteSpec::Named(rc::Rc::from(prerequisite)));
                }
                _ => {}
            }
        }

        let mut downstreams = vec![Vec::<TaskHandle>::new(); specs.len()];

        let specs: Vec<_> = specs
            .into_iter()
            .enumerate()
            .map(|(s, (spec, offset))| {
                let mut resolve_prequisite = |prerequisite| {
                    let prerequisite = match prerequisite {
                        PrerequisiteSpec::Handle(handle) => {
                            Prerequisite::Handle(handle.resolve(offset))
                        }
                        PrerequisiteSpec::Named(name) => match target_lut.get(&name) {
                            Some(index) => Prerequisite::Handle(TaskHandle::new(*index)),
                            None => Prerequisite::Named(name),
                        },
                    };
                    if let Prerequisite::Handle(handle) = prerequisite {
                        downstreams[handle.index].push(TaskHandle::new(s));
                    };
                    prerequisite
                };

                let (inputs, mut upstream): (Vec<_>, Vec<_>) = spec
                    .consumes
                    .into_iter()
                    .map(|prerequisite| {
                        let prerequisite = resolve_prequisite(prerequisite);
                        (
                            match &prerequisite {
                                Prerequisite::Handle(handle) => targets[handle.index].clone(),
                                Prerequisite::Named(path) => path.clone(),
                            },
                            prerequisite,
                        )
                    })
                    .unzip();

                upstream.extend(
                    spec.depends_on
                        .into_iter()
                        .map(|prerequisite| resolve_prequisite(prerequisite)),
                );
                upstream.extend(
                    spec.not_before
                        .into_iter()
                        .map(|prerequisite| resolve_prequisite(prerequisite)),
                );

                (targets[s].clone(), inputs, upstream, spec.env, spec.recipe)
            })
            .collect();

        drop(target_lut);
        drop(targets);

        // Combine each task spec with it's corresponding list of downstreams.
        let mut unordered_tasks: Vec<_> = specs
            .into_iter()
            .zip(downstreams)
            .map(|((target, inputs, upstream, env, recipe), downstream)| {
                Some(Task {
                    target,
                    inputs,
                    upstream,
                    downstream,
                    env,
                    recipe,
                })
            })
            .collect();

        // Extract any leaf tasks that have no upstream tasks.
        let mut tasks: Vec<_> = unordered_tasks
            .iter_mut()
            .filter_map(|task| {
                if task
                    .as_ref()
                    .unwrap()
                    .upstream
                    .iter()
                    .any(|upstream| match upstream {
                        Prerequisite::Handle(_) => true,
                        _ => false,
                    })
                {
                    None
                } else {
                    Some(task.take().unwrap())
                }
            })
            .collect();

        // Walk down from the leaf tasks to generate a list where all
        // downstream tasks appear after their upstream counterparts.
        let (mut s, mut e) = (0, tasks.len());
        while s < tasks.len() {
            for i in s..e {
                let downstream_indices: Vec<_> = tasks[i]
                    .downstream
                    .iter()
                    .map(|downstream| downstream.index)
                    .collect();
                for downstream_index in downstream_indices {
                    if let Some(downstream_task) = unordered_tasks[downstream_index].take() {
                        tasks.push(downstream_task);
                    }
                }
            }
            s = e + 1;
            e = tasks.len();
        }
        drop(unordered_tasks);

        Self { tasks }
    }

    pub fn retain_out_of_date(&self) -> Result<Vec<(TaskHandle, &Task)>, CakeError> {
        let now = SystemTime::now();

        let mut modification_times: Vec<Option<SystemTime>> = Vec::with_capacity(self.tasks.len());

        self.tasks
            .iter()
            .enumerate()
            .filter_map(
                |(index, task)| -> Option<Result<(TaskHandle, &Task), CakeError>> {
                    let upstream_mod_time = task
                        .upstream
                        .iter()
                        .filter_map(|prerequisite| {
                            let modified_time =
                                |file: &path::Path| -> Result<SystemTime, CakeError> {
                                    Ok(fs::metadata(file)
                                        .map_err(|err| {
                                            CakeError::PrerequisiteMissing(file.to_path_buf(), err)
                                        })?
                                        .modified()
                                        .map_err(|err| {
                                            CakeError::NoLastModifiedTime(file.to_path_buf(), err)
                                        })?)
                                };
                            match prerequisite {
                                Prerequisite::Named(file) => Some(modified_time(&file)),
                                Prerequisite::Handle(handle) => {
                                    modification_times[handle.index].map(|time| Ok(time))
                                }
                            }
                        })
                        .try_fold(None, |r, t| -> Result<Option<SystemTime>, CakeError> {
                            let t = t?;
                            Ok(Some(if let Some(r) = r {
                                std::cmp::max(t, r)
                            } else {
                                t
                            }))
                        });

                    let upstream_mod_time = match upstream_mod_time {
                        Ok(time) => time,
                        Err(err) => return Some(Err(err)),
                    };

                    let target_mod_time = match fs::metadata(&task.target) {
                        Ok(md) => match md.modified() {
                            Ok(time) => Some(time),
                            Err(err) => {
                                return Some(Err(CakeError::NoLastModifiedTime(
                                    task.target.to_path_buf(),
                                    err,
                                )))
                            }
                        },
                        Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => None,
                        Err(err) => {
                            return Some(Err(CakeError::IoError(task.target.to_path_buf(), err)))
                        }
                    };

                    let (mod_time, r) = match (target_mod_time, upstream_mod_time) {
                        (Some(target), Some(upstream)) => {
                            if upstream > target {
                                (Some(now), Some(Ok((TaskHandle::new(index), task))))
                            } else {
                                (Some(target), None)
                            }
                        }
                        (Some(target), None) => (Some(target), None),
                        (None, _) => (Some(now), Some(Ok((TaskHandle::new(index), task)))),
                    };

                    modification_times.push(mod_time);

                    r
                },
            )
            .collect()
    }
}

impl IntoIterator for TaskList {
    type Item = Task;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.tasks.into_iter()
    }
}

#[derive(Debug, failure::Fail)]
pub enum ParseUnitError {
    #[fail(display = "Failed to relativise a path")]
    RelativiseError(#[fail(cause)] RelativiseError),
    #[fail(display = "I/O Error while parsing unit")]
    IoError(#[fail(cause)] std::io::Error),
    #[fail(display = "Error while parsing unit")]
    Other(#[fail(cause)] failure::Error),
}

impl From<RelativiseError> for ParseUnitError {
    fn from(err: RelativiseError) -> Self {
        Self::RelativiseError(err)
    }
}

impl From<std::io::Error> for ParseUnitError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<failure::Error> for ParseUnitError {
    fn from(err: failure::Error) -> Self {
        Self::Other(failure::Error::from(err))
    }
}

pub trait FrontEnd {
    fn parse_unit<'v, 'p>(
        &self,
        file: &path::Path,
        unit_builder: UnitBuilder<'v, 'p>,
    ) -> Result<Unit, ParseUnitError>;
}

#[derive(Debug, failure::Fail)]
pub enum GatherUnitsError {
    #[fail(
        display = "Failed to process '{}': No front-end for '{}' files.",
        file, ext
    )]
    NoFrontEnd { file: String, ext: String },
    #[fail(display = "Unable to find a unit")]
    NoSuchUnit,
    #[fail(display = "Failed to parse '{}'", file)]
    ParseError {
        file: String,
        #[fail(cause)]
        cause: ParseUnitError,
    },
}

pub struct Engine {
    frontends: collections::HashMap<ffi::OsString, Box<dyn FrontEnd>>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            frontends: std::collections::HashMap::new(),
        }
    }

    pub fn register_frontend<F>(&mut self, ext: &str, f: F)
    where
        F: FrontEnd + 'static,
    {
        self.frontends.insert(ext.into(), Box::new(f));
    }

    pub fn gather_units(&self, context: &path::Path) -> Result<Vec<Unit>, GatherUnitsError> {
        for (ext, frontend) in self.frontends.iter() {
            let file = context.join("asmbl").with_extension(ext);
            if file.exists() {
                let mut units = vec![];
                let context: Vec<_> = context.components().collect();
                self.parse_unit(&context, &file, frontend, &mut units)?;
                return Ok(units);
            }
        }
        Err(GatherUnitsError::NoSuchUnit)
    }

    fn parse_unit<'v, 'p>(
        &self,
        context: &'v Vec<path::Component<'p>>,
        file: &path::Path,
        frontend: &Box<dyn FrontEnd>,
        units: &mut Vec<Unit>,
    ) -> Result<(), GatherUnitsError> {
        // FIXME Do better than unwrap!!!
        let unit_builder = UnitBuilder::new(context, file.parent().unwrap().to_path_buf());

        let unit = frontend.parse_unit(&file, unit_builder).map_err(|err| {
            GatherUnitsError::ParseError {
                file: file.to_string_lossy().into_owned(),
                cause: err,
            }
        })?;

        for (file, _optional) in unit.sub_units.iter() {
            self.load_unit(context, &file, units)?;
        }

        units.push(unit);

        Ok(())
    }

    fn load_unit<'v, 'p>(
        &self,
        context: &'v Vec<path::Component<'p>>,
        file: &path::Path,
        units: &mut Vec<Unit>,
    ) -> Result<(), GatherUnitsError> {
        let ext = file.extension().unwrap_or(ffi::OsStr::new(""));

        let frontend = self
            .frontends
            .get(ext)
            .ok_or(GatherUnitsError::NoFrontEnd {
                file: file.to_string_lossy().into_owned(),
                ext: ext.to_string_lossy().into_owned(),
            })?;

        self.parse_unit(context, file, &frontend, units)?;

        Ok(())
    }
}
