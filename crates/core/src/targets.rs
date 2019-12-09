use std::{path, rc};

use crate::targets_spec::{TargetSpec, TargetsSpec};

#[derive(Clone, Debug)]
pub enum Targets {
    Single(rc::Rc<path::Path>),
    Multi(Vec<rc::Rc<path::Path>>),
}

impl Targets {
    pub fn iter(&self) -> TargetIterator {
        match self {
            Self::Single(path) => TargetIterator::Single(Some(path)),
            Self::Multi(paths) => TargetIterator::Multi(paths.iter()),
        }
    }

    pub fn try_from(
        (prefix, input, spec): (path::PathBuf, &Option<rc::Rc<path::Path>>, TargetsSpec),
    ) -> Result<Self, crate::targets_spec::ResolveError> {
        let resolve_spec = |prefix: path::PathBuf, spec: TargetSpec| {
            Ok(rc::Rc::from(
                spec.resolve(prefix, input.as_ref().map(|i| i.as_ref()))?,
            ))
        };

        Ok(match spec {
            TargetsSpec::Single(spec) => Self::Single(resolve_spec(prefix, spec)?),
            TargetsSpec::Multi(specs) => Self::Multi(
                specs
                    .into_iter()
                    // FIXME The first/last one doesn't need to be a clone
                    .map(|spec| resolve_spec(prefix.clone(), spec))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        })
    }
}

impl std::ops::Index<usize> for Targets {
    type Output = rc::Rc<path::Path>;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::Single(path) if index == 0 => path,
            Self::Multi(paths) => &paths[index],
            _ => panic!(),
        }
    }
}

impl IntoIterator for Targets {
    type Item = rc::Rc<path::Path>;
    type IntoIter = TargetIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Single(path) => TargetIntoIterator::Single(Some(path)),
            Self::Multi(paths) => TargetIntoIterator::Multi(paths.into_iter()),
        }
    }
}

pub enum TargetIterator<'a> {
    Single(Option<&'a rc::Rc<path::Path>>),
    Multi(std::slice::Iter<'a, rc::Rc<path::Path>>),
}

impl<'a> Iterator for TargetIterator<'a> {
    type Item = &'a rc::Rc<path::Path>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Single(path) => path.take(),
            Self::Multi(iter) => iter.next(),
        }
    }
}

pub enum TargetIntoIterator {
    Single(Option<rc::Rc<path::Path>>),
    Multi(std::vec::IntoIter<rc::Rc<path::Path>>),
}

impl Iterator for TargetIntoIterator {
    type Item = rc::Rc<path::Path>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Single(path) => path.take(),
            Self::Multi(iter) => iter.next(),
        }
    }
}
