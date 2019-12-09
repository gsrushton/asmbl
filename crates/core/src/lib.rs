use std::{collections, ffi, fs, path, rc, time::SystemTime};

mod env;
mod make;
mod recipe;
mod relativiser;
mod targets;
mod targets_spec;
mod unit;

use targets::Targets;

pub use env::EnvSpec;
pub use recipe::Recipe;
pub use relativiser::Error;
pub use targets_spec::{TargetSpec, TargetsSpec};
pub use unit::{
    PrerequisiteSpec, TargetSpecHandle, TargetSpecHandleIterator, TaskSpec, Unit, UnitBuilder,
};

#[derive(Debug)]
enum Prerequisite {
    Named(rc::Rc<path::Path>, bool),
    Handle(TaskHandle),
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
    targets: Targets,
    inputs: Vec<rc::Rc<path::Path>>,
    upstream: Vec<Prerequisite>,
    downstream: Vec<TaskHandle>,
    env: Vec<EnvSpec>,
    recipe: Recipe,
}

impl Task {
    // TODO wouldn't it be nice if the was self
    pub fn prepare(&self) -> Result<std::process::Command, recipe::RecipePrepareError> {
        self.recipe.prepare(&self.targets, &self.inputs, &self.env)
    }
}

#[derive(Debug)]
pub struct TaskList {
    tasks: Vec<Task>,
}

#[derive(Debug, failure::Fail)]
pub enum NewTaskListError {
    #[fail(display = "Failed to resolve target")]
    ResolveError(#[fail(cause)] targets_spec::ResolveError),
    #[fail(display = "Failed to relativise a path")]
    RelativiseError(#[fail(cause)] relativiser::Error),
    #[fail(display = "Failed to parse make file")]
    MakeParseError(#[fail(cause)] make::ParserError),
    #[fail(display = "IO Error")]
    IOError(#[fail(cause)] std::io::Error)
}

impl From<targets_spec::ResolveError> for NewTaskListError {
    fn from(err: targets_spec::ResolveError) -> Self {
        Self::ResolveError(err)
    }
}

impl From<relativiser::Error> for NewTaskListError {
    fn from(err: relativiser::Error) -> Self {
        Self::RelativiseError(err)
    }
}

impl From<make::ParserError> for NewTaskListError {
    fn from(err: make::ParserError) -> Self {
        Self::MakeParseError(err)
    }
}

impl From<std::io::Error> for NewTaskListError {
    fn from(err: std::io::Error) -> Self {
        Self::IOError(err)
    }
}

impl TaskList {
    pub fn new<I>(
        context_dir: &path::Path,
        target_prefix: &path::Path,
        units: I,
    ) -> Result<Self, NewTaskListError>
    where
        I: IntoIterator<Item = (path::PathBuf, Unit)>,
    {
        let context: Vec<_> = context_dir.components().collect();

        // Extract the list of tasks from each unit,
        // flattening them into one big list.

        let (cakes, includes): (Vec<_>, Vec<_>) = units
            .into_iter()
            .map(|(dir, unit)| (dir, unit.decompose()))
            .scan(0, |count, (dir, (task_specs, includes))| {
                let offset = *count;
                *count += task_specs.len();
                let task_specs = task_specs
                    .into_iter()
                    .map(move |(targets_spec, task_spec)| {
                        (Some(targets_spec), task_spec.resolve(offset))
                    });
                let includes = includes
                    .into_iter()
                    .map(move |include| include.resolve(offset));

                Some((task_specs, (dir, includes)))
            })
            .unzip();

        let (mut targets_specs, mut task_specs): (Vec<_>, Vec<_>) = cakes.into_iter().flatten().unzip();

        let mut targets: Vec<Option<Targets>> = vec![None; targets_specs.len()];

        fn something<'a>(
            task_index: usize,
            target_prefix: &path::Path,
            targets: &mut Vec<Option<Targets>>,
            targets_specs: &mut Vec<Option<TargetsSpec>>,
            task_specs: &'a Vec<TaskSpec<rc::Rc<path::Path>>>,
        ) -> Result<Option<rc::Rc<path::Path>>, crate::targets_spec::ResolveError> {
            if let Some(targets_spec) = targets_specs[task_index].take() {
                let input = task_specs[task_index]
                    .consumes
                    .first()
                    .map(|p| match p {
                        PrerequisiteSpec::Handle(handle) => something(
                            handle.task_index,
                            target_prefix,
                            targets,
                            targets_specs,
                            task_specs,
                        ),
                        PrerequisiteSpec::Named(path, _) => Ok(Some(path.clone())),
                    })
                    .map_or(Ok(None), |r| r)?;

                targets[task_index] = Some(Targets::try_from((
                    target_prefix.to_path_buf(),
                    &input,
                    targets_spec,
                ))?);

                Ok(input)
            } else {
                Ok(Some(targets[task_index].as_ref().unwrap()[0].clone()))
            }
        };

        for task_index in 0..targets_specs.len() {
            something(
                task_index,
                target_prefix,
                &mut targets,
                &mut targets_specs,
                &task_specs,
            )?;
        }
        drop(targets_specs);

        // Build a flat list of files and a map from
        // file-path to index within that list.
        let target_lut: collections::HashMap<_, _> = targets
            .iter()
            .enumerate()
            .map(|(task_index, target)| {
                target
                    .as_ref()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(move |(target_index, path)| (path.clone(), (task_index, target_index)))
            })
            .flatten()
            .collect();

        // Account for any extra prerequisites.
        let get_target = |handle: TargetSpecHandle| {
            let target = targets[handle.task_index].as_ref().unwrap();
            &target[handle.target_index]
        };

        for (dir, includes) in includes.into_iter() {
            let relativiser = relativiser::Relativiser::new(dir);
            for include in includes {
                let content = asmbl_utils::io::read_file(fs::File::open(get_target(include))?)?;

                for (target, prerequisite) in make::cake(&content)? {

                    let target = relativiser.relativise(&context, path::Path::new(target))?;
                    let prerequisite = relativiser.relativise(&context, path::Path::new(prerequisite))?;

                    match target_lut.get(&rc::Rc::from(target)) {
                        Some((task_index, _)) => {
                            task_specs[*task_index]
                                .depends_on
                                .push(PrerequisiteSpec::Named(rc::Rc::from(prerequisite), true));
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut downstreams = vec![Vec::<TaskHandle>::new(); task_specs.len()];

        let task_specs: Vec<_> = task_specs
            .into_iter()
            .enumerate()
            .map(|(s, task_spec)| {
                let mut resolve_prequisite =
                    |prerequisite: PrerequisiteSpec<rc::Rc<path::Path>>| {
                        let (prerequisite, path) = match prerequisite {
                            PrerequisiteSpec::Handle(handle) => (
                                Prerequisite::Handle(TaskHandle::new(handle.task_index)),
                                get_target(handle).clone(),
                            ),
                            PrerequisiteSpec::Named(name, optional) => match target_lut.get(&name) {
                                Some((task_index, target_index)) => (
                                    Prerequisite::Handle(TaskHandle::new(*task_index)),
                                    targets[*task_index].as_ref().unwrap()[*target_index].clone(),
                                ),
                                None => (Prerequisite::Named(name.clone(), optional), name),
                            },
                        };
                        if let Prerequisite::Handle(handle) = prerequisite {
                            downstreams[handle.index].push(TaskHandle::new(s));
                        };
                        (prerequisite, path)
                    };

                let (mut upstream, inputs): (Vec<_>, Vec<_>) = task_spec
                    .consumes
                    .into_iter()
                    .map(|prerequisite| resolve_prequisite(prerequisite))
                    .unzip();

                upstream.extend(
                    task_spec
                        .depends_on
                        .into_iter()
                        .map(|prerequisite| resolve_prequisite(prerequisite).0),
                );
                upstream.extend(
                    task_spec
                        .not_before
                        .into_iter()
                        .map(|prerequisite| resolve_prequisite(prerequisite).0),
                );

                (inputs, upstream, task_spec.env, task_spec.recipe)
            })
            .collect();

        drop(target_lut);

        // Combine each task spec with it's corresponding list of downstreams.
        let mut unordered_tasks: Vec<_> = targets
            .into_iter()
            .zip(task_specs)
            .zip(downstreams)
            .map(
                |((mut targets, (inputs, upstream, env, recipe)), downstream)| {
                    Some(Task {
                        targets: targets.take().unwrap(),
                        inputs,
                        upstream,
                        downstream,
                        env,
                        recipe,
                    })
                },
            )
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

        Ok(Self { tasks })
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
                            match prerequisite {
                                Prerequisite::Named(file, optional) => match fs::metadata(&file) {
                                    Ok(metadata) => {
                                        Some(metadata.modified()
                                        .map_err(|err| {
                                            CakeError::NoLastModifiedTime(file.to_path_buf(), err)
                                        }))
                                    },
                                    Err(_) if *optional => None,
                                    Err(err) => Some(Err(CakeError::PrerequisiteMissing(file.to_path_buf(), err)))
                                },
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

                    let target_mod_time = task
                        .targets
                        .iter()
                        .map(|target| match fs::metadata(&target) {
                            Ok(md) => match md.modified() {
                                Ok(time) => Ok(Some(time)),
                                Err(err) => {
                                    return Err(CakeError::NoLastModifiedTime(
                                        target.to_path_buf(),
                                        err,
                                    ))
                                }
                            },
                            Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                            Err(err) => return Err(CakeError::IoError(target.to_path_buf(), err)),
                        })
                        .try_fold(
                            None,
                            |r, t| -> Result<Option<Option<SystemTime>>, CakeError> {
                                let t = t?;
                                Ok(Some(if let Some(r) = r {
                                    match (r, t) {
                                        (None, _) | (_, None) => None,
                                        (Some(r), Some(t)) => Some(std::cmp::max(t, r)),
                                    }
                                } else {
                                    t
                                }))
                            },
                        );

                    let target_mod_time = match target_mod_time {
                        Ok(time) => time.unwrap_or(None),
                        Err(err) => return Some(Err(err)),
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
    RelativiseError(#[fail(cause)] relativiser::Error),
    #[fail(display = "Unit not found")]
    NotFound,
    #[fail(display = "I/O Error while parsing unit")]
    IoError(#[fail(cause)] std::io::Error),
    #[fail(display = "Error while parsing unit")]
    Other(#[fail(cause)] failure::Error),
}

impl From<relativiser::Error> for ParseUnitError {
    fn from(err: relativiser::Error) -> Self {
        Self::RelativiseError(err)
    }
}

impl From<std::io::Error> for ParseUnitError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::IoError(err),
        }
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
        path: &path::Path,
        unit_builder: UnitBuilder<'v, 'p>,
    ) -> Result<Unit, ParseUnitError>;
}

#[derive(Debug, failure::Fail)]
pub enum GatherUnitsError {
    #[fail(display = "No such root unit")]
    NoRootUnit,
    #[fail(display = "Bad sub-unit: '{}'.", file)]
    BadSubUnit { file: String },
    #[fail(display = "No front-end for '{}'.", file)]
    NoFrontEnd { file: String, ext: String },
    #[fail(display = "Sub-unit '{}' not under context.", file)]
    UnitNotInContext { file: String },
    #[fail(display = "Failed to parse '{}'.", file)]
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

    pub fn gather_units(
        &self,
        dir: &path::Path
    ) -> Result<Vec<(path::PathBuf, Unit)>, GatherUnitsError> {
        for (ext, frontend) in self.frontends.iter() {
            let file = dir.join("asmbl").with_extension(ext);
            if file.exists() {
                let mut units = vec![];
                let context: Vec<_> = dir.components().collect();
                self.parse_unit(&context, dir, &file, frontend, &mut units)?;
                return Ok(units);
            }
        }
        Err(GatherUnitsError::NoRootUnit)
    }

    fn parse_unit<'v, 'p>(
        &self,
        context: &'v Vec<path::Component<'p>>,
        dir: &path::Path,
        file: &path::Path,
        frontend: &Box<dyn FrontEnd>,
        units: &mut Vec<(path::PathBuf, Unit)>,
    ) -> Result<(), GatherUnitsError> {
        let unit_builder = UnitBuilder::new(context, dir.to_path_buf());

        match frontend.parse_unit(&file, unit_builder) {
            Ok(unit) => {
                for sub_unit in unit.sub_units.iter() {
                    let ext = sub_unit.extension().unwrap_or(ffi::OsStr::new(""));

                    let frontend = self
                        .frontends
                        .get(ext)
                        .ok_or(GatherUnitsError::NoFrontEnd {
                            file: sub_unit.to_string_lossy().into_owned(),
                            ext: ext.to_string_lossy().into_owned(),
                        })?;

                    self.parse_unit(
                        context,
                        sub_unit
                            .parent()
                            .ok_or_else(|| GatherUnitsError::BadSubUnit {
                                file: sub_unit.to_string_lossy().into_owned(),
                            })?,
                        &file,
                        &frontend,
                        units,
                    )?;
                }

                units.push((dir.to_path_buf(), unit));

                Ok(())
            }
            Err(err) => Err(GatherUnitsError::ParseError {
                file: file.to_string_lossy().into_owned(),
                cause: err,
            }),
        }
    }
}
