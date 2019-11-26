use std::{collections, ffi, fs, path, rc, time::SystemTime};

mod env;
mod recipe;
mod targets;
mod targets_spec;

use targets::Targets;

pub use env::EnvSpec;
pub use recipe::Recipe;
pub use targets_spec::TargetsSpec;

#[derive(Debug)]
enum Prerequisite {
    Named(rc::Rc<path::Path>),
    Handle(TaskHandle),
}

pub enum PrerequisiteSpec {
    Named(rc::Rc<path::Path>),
    Handle(TargetSpecHandle),
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
pub struct TargetSpecHandle {
    task_index: usize,
    target_index: usize,
}

impl TargetSpecHandle {
    fn new(task_index: usize, target_index: usize) -> Self {
        Self {
            task_index,
            target_index,
        }
    }

    fn resolve(self, task_offset: usize) -> TaskHandle {
        TaskHandle::new(self.task_index + task_offset)
    }
}

pub struct TargetSpecHandleIterator {
    task_index: usize,
    target_count: usize,
    target_index: usize,
}

impl TargetSpecHandleIterator {
    pub fn new(task_index: usize, target_count: usize) -> Self {
        Self {
            task_index,
            target_count,
            target_index: 0,
        }
    }
}

impl Iterator for TargetSpecHandleIterator {
    type Item = TargetSpecHandle;

    fn next(&mut self) -> Option<Self::Item> {
        if self.target_index < self.target_count {
            let handle = TargetSpecHandle::new(self.task_index, self.target_index);
            self.target_index += 1;
            Some(handle)
        } else {
            None
        }
    }
}

pub enum SubUnitSpec {
    Target(TargetSpecHandle),
    Named(path::PathBuf),
}

pub struct Unit {
    tasks: Vec<(TargetsSpec, TaskSpec)>,
    prerequisites: Vec<(path::PathBuf, path::PathBuf)>,
    sub_units: Vec<SubUnitSpec>,
}

impl Unit {
    fn new() -> Self {
        Self {
            tasks: vec![],
            prerequisites: vec![],
            sub_units: vec![],
        }
    }

    pub fn target_path(&self, handle: &TargetSpecHandle) -> &path::Path {
        &self.tasks[handle.task_index].0[handle.target_index]
    }

    pub fn add_task(
        &mut self,
        targets: TargetsSpec,
        consumes: Vec<PrerequisiteSpec>,
        depends_on: Vec<PrerequisiteSpec>,
        not_before: Vec<PrerequisiteSpec>,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> TargetSpecHandleIterator {
        let target_count = targets.len();
        let task_index = self.tasks.len();
        self.tasks.push((
            targets,
            TaskSpec::new(consumes, depends_on, not_before, env, recipe),
        ));
        TargetSpecHandleIterator::new(task_index, target_count)
    }

    pub fn add_prerequisite(&mut self, target: path::PathBuf, prerequisite: path::PathBuf) {
        self.prerequisites.push((target, prerequisite))
    }

    pub fn add_sub_unit(&mut self, sub_unit: SubUnitSpec) {
        self.sub_units.push(sub_unit)
    }

    pub fn decompose(
        self,
    ) -> (
        Vec<(TargetsSpec, TaskSpec)>,
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
    // FIXME This could be a reference...
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
        targets: TargetsSpec,
        consumes: Vec<PrerequisiteSpec>,
        depends_on: Vec<PrerequisiteSpec>,
        not_before: Vec<PrerequisiteSpec>,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> Result<TargetSpecHandleIterator, RelativiseError> {
        let targets = targets.map(|path| self.relativise(&path))?;

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
            .add_task(targets, consumes, depends_on, not_before, env, recipe))
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

    pub fn add_sub_unit(&mut self, sub_unit: SubUnitSpec) -> Result<(), RelativiseError> {
        Ok(self.unit.add_sub_unit(match sub_unit {
            SubUnitSpec::Named(file) => SubUnitSpec::Named(self.relativise(&file)?),
            _ => sub_unit,
        }))
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

impl TaskList {
    pub fn new<I>(target_prefix: &path::Path, units: I) -> Self
    where
        I: IntoIterator<Item = Unit>,
    {
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
                    (Targets::from((target_prefix, target)), (spec, offset))
                }))
            })
            .flatten()
            .unzip();

        // Build a flat list of files and a map from
        // file-path to index within that list.
        let target_lut: collections::HashMap<_, _> = targets
            .iter()
            .enumerate()
            .map(|(task_index, target)| {
                target
                    .iter()
                    .enumerate()
                    .map(move |(target_index, path)| (path.clone(), (task_index, target_index)))
            })
            .flatten()
            .collect();

        // Account for any extra prerequisites.
        for (target, prerequisite) in prerequisites.into_iter().flatten() {
            match target_lut.get(&rc::Rc::from(target)) {
                Some((task_index, _)) => {
                    specs[*task_index]
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
                    let (prerequisite, path) = match prerequisite {
                        PrerequisiteSpec::Handle(handle) => (
                            Prerequisite::Handle(handle.resolve(offset)),
                            targets[handle.task_index][handle.target_index].clone(),
                        ),
                        PrerequisiteSpec::Named(name) => match target_lut.get(&name) {
                            Some((task_index, target_index)) => (
                                Prerequisite::Handle(TaskHandle::new(*task_index)),
                                targets[*task_index][*target_index].clone(),
                            ),
                            None => {
                                let path = rc::Rc::from(name) as rc::Rc<path::Path>;
                                (Prerequisite::Named(path.clone()), path)
                            }
                        },
                    };
                    if let Prerequisite::Handle(handle) = prerequisite {
                        downstreams[handle.index].push(TaskHandle::new(s));
                    };
                    (prerequisite, path)
                };

                let (mut upstream, inputs): (Vec<_>, Vec<_>) = spec
                    .consumes
                    .into_iter()
                    .map(|prerequisite| resolve_prequisite(prerequisite))
                    .unzip();

                upstream.extend(
                    spec.depends_on
                        .into_iter()
                        .map(|prerequisite| resolve_prequisite(prerequisite).0),
                );
                upstream.extend(
                    spec.not_before
                        .into_iter()
                        .map(|prerequisite| resolve_prequisite(prerequisite).0),
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
            .map(|((targets, inputs, upstream, env, recipe), downstream)| {
                Some(Task {
                    targets,
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
    RelativiseError(#[fail(cause)] RelativiseError),
    #[fail(display = "Unit not found")]
    NotFound,
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
        dir: &path::Path,
        target_prefix: &path::Path,
    ) -> Result<Vec<(path::PathBuf, Unit)>, GatherUnitsError> {
        for (ext, frontend) in self.frontends.iter() {
            let file = dir.join("asmbl").with_extension(ext);
            if file.exists() {
                let mut units = vec![];
                let context: Vec<_> = dir.components().collect();
                self.parse_unit(
                    &context,
                    target_prefix,
                    dir,
                    &file,
                    false,
                    frontend,
                    &mut units,
                )?;
                return Ok(units);
            }
        }
        Err(GatherUnitsError::NoRootUnit)
    }

    fn parse_unit<'v, 'p>(
        &self,
        context: &'v Vec<path::Component<'p>>,
        target_prefix: &path::Path,
        dir: &path::Path,
        file: &path::Path,
        optional: bool,
        frontend: &Box<dyn FrontEnd>,
        units: &mut Vec<(path::PathBuf, Unit)>,
    ) -> Result<(), GatherUnitsError> {
        let unit_builder = UnitBuilder::new(context, dir.to_path_buf());

        match frontend.parse_unit(&file, unit_builder) {
            Ok(unit) => {
                for sub_unit in unit.sub_units.iter() {
                    // TODO Could be a Cow
                    let (file, include, optional) = match sub_unit {
                        SubUnitSpec::Target(handle) => {
                            (target_prefix.join(unit.target_path(handle)), true, true)
                        }
                        SubUnitSpec::Named(file) => (file.to_path_buf(), false, false),
                    };

                    let ext = file.extension().unwrap_or(ffi::OsStr::new(""));

                    let frontend = self
                        .frontends
                        .get(ext)
                        .ok_or(GatherUnitsError::NoFrontEnd {
                            file: file.to_string_lossy().into_owned(),
                            ext: ext.to_string_lossy().into_owned(),
                        })?;

                    // TODO Could be a Cow
                    let dir = if include {
                        dir
                    } else {
                        file.parent().ok_or_else(|| GatherUnitsError::BadSubUnit {
                            file: file.to_string_lossy().into_owned(),
                        })?
                    };

                    self.parse_unit(
                        context,
                        target_prefix,
                        dir,
                        &file,
                        optional,
                        &frontend,
                        units,
                    )?;
                }

                units.push((dir.to_path_buf(), unit));

                Ok(())
            }
            Err(err) => match err {
                ParseUnitError::NotFound if optional => Ok(()),
                _ => Err(GatherUnitsError::ParseError {
                    file: file.to_string_lossy().into_owned(),
                    cause: err,
                }),
            },
        }
    }
}
