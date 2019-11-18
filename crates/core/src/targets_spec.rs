use std::path;

pub enum TargetsSpec {
    Single(path::PathBuf),
    Multi(Vec<path::PathBuf>),
}

impl TargetsSpec {
    pub fn len(&self) -> usize {
        match self {
            Self::Single(_) => 1,
            Self::Multi(targets) => targets.len(),
        }
    }

    pub fn map<F, E>(self, mut f: F) -> Result<Self, E>
    where
        F: FnMut(path::PathBuf) -> Result<path::PathBuf, E>,
    {
        Ok(match self {
            Self::Single(path) => Self::Single(f(path)?),
            Self::Multi(paths) => Self::Multi(
                paths
                    .into_iter()
                    .map(|path| f(path))
                    .collect::<Result<Vec<_>, E>>()?,
            ),
        })
    }
}

impl std::ops::Index<usize> for TargetsSpec {
    type Output = path::Path;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::Single(path) if index == 0 => path,
            Self::Multi(paths) => &paths[index],
            _ => panic!(),
        }
    }
}
