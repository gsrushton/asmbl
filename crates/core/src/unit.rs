use crate::env::EnvSpec;
use crate::recipe::Recipe;
use crate::relativiser;
use crate::targets_spec::TargetsSpec;

use std::{path, rc};

#[derive(Clone)]
pub enum PrerequisiteSpec<Path> {
    Named(Path, bool),
    Handle(TargetSpecHandle),
}

impl PrerequisiteSpec<path::PathBuf> {
    pub fn resolve(self, offset: usize) -> PrerequisiteSpec<rc::Rc<path::Path>> {
        match self {
            Self::Named(path, optional) => {
                PrerequisiteSpec::Named(rc::Rc::from(path) as rc::Rc<path::Path>, optional)
            }
            Self::Handle(handle) => PrerequisiteSpec::Handle(handle.resolve(offset)),
        }
    }
}

#[derive(Clone, Copy)]
pub struct TargetSpecHandle {
    pub task_index: usize,
    pub target_index: usize,
}

impl TargetSpecHandle {
    pub fn new(task_index: usize, target_index: usize) -> Self {
        Self {
            task_index,
            target_index,
        }
    }

    pub fn resolve(self, task_offset: usize) -> Self {
        Self {
            task_index: self.task_index + task_offset,
            target_index: self.target_index,
        }
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

pub struct TaskSpec<Path> {
    pub consumes: Vec<PrerequisiteSpec<Path>>,
    pub depends_on: Vec<PrerequisiteSpec<Path>>,
    pub not_before: Vec<PrerequisiteSpec<Path>>,
    pub aggregate: bool,
    pub env: Vec<EnvSpec>,
    pub recipe: Recipe,
}

impl<Path> TaskSpec<Path> {
    pub fn decompose(
        self,
    ) -> (
        Vec<PrerequisiteSpec<Path>>,
        Vec<PrerequisiteSpec<Path>>,
        Vec<PrerequisiteSpec<Path>>,
        Vec<EnvSpec>,
        Recipe,
    ) {
        (
            self.consumes,
            self.depends_on,
            self.not_before,
            self.env,
            self.recipe,
        )
    }
}

impl TaskSpec<path::PathBuf> {
    fn new(
        consumes: Vec<PrerequisiteSpec<path::PathBuf>>,
        depends_on: Vec<PrerequisiteSpec<path::PathBuf>>,
        not_before: Vec<PrerequisiteSpec<path::PathBuf>>,
        aggregate: bool,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> Self {
        Self {
            consumes,
            depends_on,
            not_before,
            aggregate,
            env,
            recipe: recipe,
        }
    }

    pub fn resolve(self, offset: usize) -> TaskSpec<rc::Rc<path::Path>> {
        let resolve_prequisites = |prerequisites: Vec<PrerequisiteSpec<path::PathBuf>>| {
            prerequisites
                .into_iter()
                .map(|prerequisite| prerequisite.resolve(offset))
                .collect()
        };
        TaskSpec {
            consumes: resolve_prequisites(self.consumes),
            depends_on: resolve_prequisites(self.depends_on),
            not_before: resolve_prequisites(self.not_before),
            aggregate: self.aggregate,
            env: self.env,
            recipe: self.recipe,
        }
    }
}

pub struct TaskSet<Path> {
    tasks: Vec<(TargetsSpec, TaskSpec<Path>)>,
}

impl<Path> TaskSet<Path> {
    pub fn single(targets: TargetsSpec, task_spec: TaskSpec<Path>) -> Self {
        Self {
            tasks: vec![(targets, task_spec)]
        }
    }
}

impl TaskSet<path::PathBuf> {
    pub fn resolve(self, offset: usize) -> TaskSet<rc::Rc<path::Path>> {
        self.tasks.into_iter().map(|(targets, task_spec)| (targets, task_spec.resolve(offset))).collect()
    }
}

impl<Path> std::iter::FromIterator<(TargetsSpec, TaskSpec<Path>)> for TaskSet<Path> {
    fn from_iter<T>(iter: T) -> Self
        where T: IntoIterator<Item = (TargetsSpec, TaskSpec<Path>)>
    {
        Self {
            tasks: iter.into_iter().collect()
        }
    }
}

pub struct Unit {
    tasks: Vec<(TargetsSpec, TaskSpec<path::PathBuf>)>,
    includes: Vec<TargetSpecHandle>,
    pub sub_units: Vec<path::PathBuf>,
}

impl Unit {
    fn new() -> Self {
        Self {
            tasks: vec![],
            includes: vec![],
            sub_units: vec![],
        }
    }

    fn add_task(
        &mut self,
        targets: TargetsSpec,
        consumes: Vec<PrerequisiteSpec<path::PathBuf>>,
        depends_on: Vec<PrerequisiteSpec<path::PathBuf>>,
        not_before: Vec<PrerequisiteSpec<path::PathBuf>>,
        aggregate: bool,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> TargetSpecHandleIterator {
        let target_count = targets.len();
        let task_index = self.tasks.len();
        self.tasks.push((
            targets,
            TaskSpec::new(consumes, depends_on, not_before, aggregate, env, recipe),
        ));
        TargetSpecHandleIterator::new(task_index, target_count)
    }

    fn add_include(&mut self, include: TargetSpecHandle) {
        self.includes.push(include)
    }

    fn add_sub_unit(&mut self, sub_unit: path::PathBuf) {
        self.sub_units.push(sub_unit)
    }

    pub fn decompose(
        self,
    ) -> (
        Vec<TaskSet<path::PathBuf>>,
        Vec<TargetSpecHandle>,
    ) {
        let task_sets = self.tasks.into_iter().map(|(targets, task)| {
            if task.aggregate || task.consumes.len() <= 1 {
                TaskSet::single(targets, task)
            } else {
                let (consumes, depends_on, not_before, env, recipe) = task.decompose();
                consumes.into_iter().map(|input| {
                    (
                        targets.clone(),
                        TaskSpec::new(
                            vec![input],
                            depends_on.clone(),
                            not_before.clone(),
                            true,
                            env.clone(),
                            recipe.clone(),
                        ),
                    )
                }).collect()
            }
        });
        (task_sets.collect(), self.includes)
    }
}

pub struct UnitBuilder<'p, 'v> {
    context: &'v Vec<path::Component<'p>>,
    relativiser: relativiser::Relativiser,
    unit: Unit,
}

#[derive(Debug, failure::Fail)]
pub enum AddTaskError {
    #[fail(display = "Failed to relativise a path")]
    RelativiseError(#[fail(cause)] relativiser::Error),
    #[fail(display = "Non unicode path.")]
    NonUnicodePath,
}

impl From<relativiser::Error> for AddTaskError {
    fn from(err: relativiser::Error) -> Self {
        Self::RelativiseError(err)
    }
}

impl<'p, 'v> UnitBuilder<'p, 'v> {
    pub fn new(context: &'v Vec<path::Component<'p>>, base: path::PathBuf) -> Self {
        Self {
            context,
            relativiser: relativiser::Relativiser::new(base),
            unit: Unit::new(),
        }
    }

    pub fn add_task(
        &mut self,
        targets: Vec<String>,
        consumes: Vec<PrerequisiteSpec<path::PathBuf>>,
        depends_on: Vec<PrerequisiteSpec<path::PathBuf>>,
        not_before: Vec<PrerequisiteSpec<path::PathBuf>>,
        aggregate: bool,
        env: Vec<EnvSpec>,
        recipe: Recipe,
    ) -> Result<TargetSpecHandleIterator, AddTaskError> {
        let targets = targets
            .into_iter()
            .map(|path| {
                self.relativise(path::Path::new(&path))
                    .map_err(|err| AddTaskError::from(err))
                    .and_then(|path| {
                        path.into_os_string()
                            .into_string()
                            .or(Err(AddTaskError::NonUnicodePath))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let relativise_prequisite =
            |prerequisite: PrerequisiteSpec<path::PathBuf>| -> Result<_, AddTaskError> {
                match prerequisite {
                    PrerequisiteSpec::Named(name, optional) => {
                        Ok(PrerequisiteSpec::Named(self.relativise(&name)?, optional))
                    }
                    _ => Ok(prerequisite),
                }
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

        Ok(self.unit.add_task(
            targets.into(),
            consumes,
            depends_on,
            not_before,
            aggregate,
            env,
            recipe,
        ))
    }

    pub fn add_sub_unit(&mut self, sub_unit: path::PathBuf) -> Result<(), relativiser::Error> {
        Ok(self.unit.add_sub_unit(self.relativise(&sub_unit)?))
    }

    pub fn add_include(&mut self, include: TargetSpecHandle) {
        self.unit.add_include(include)
    }

    pub fn unit(self) -> Unit {
        self.unit
    }

    fn relativise(&self, path: &path::Path) -> Result<path::PathBuf, relativiser::Error> {
        self.relativiser.relativise(self.context, path)
    }
}
