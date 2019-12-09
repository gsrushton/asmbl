use std::path;

pub struct TargetSpec {
    path: path::PathBuf,
    markers: Vec<usize>
}

impl TargetSpec {
    pub fn resolve(self, prefix: &path::Path, input: &path::Path) -> path::PathBuf {
        use std::borrow::Cow;

        let path = prefix.to_path_buf();

        let (mut p, mut m) = (0usize, 0usize);
        for component in self.path.components() {
            let component = component.as_os_str();

            let pn = p + component.len();

            let mut fragment_start = 0;
            let new_component = Cow::Borrowed(component);
            while m < self.markers.len() && self.markers[m] < pn {
                let fragment_end = pn - self.markers[m];
                let new_component = new_component.to_owned();

                new_component.push(component[fragment_start..fragment_end]);

                m += 1;
            }

            path.push(new_component);

            p = pn;
        }

        path
    }
}

pub enum TargetsSpec {
    Single(TargetSpec),
    Multi(Vec<TargetSpec>),
}

impl TargetsSpec {
    pub fn len(&self) -> usize {
        match self {
            Self::Single(_) => 1,
            Self::Multi(targets) => targets.len(),
        }
    }

    /*pub fn map<F, E>(self, mut f: F) -> Result<Self, E>
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
    }*/
}

impl std::ops::Index<usize> for TargetsSpec {
    type Output = TargetSpec;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::Single(path) if index == 0 => path,
            Self::Multi(paths) => &paths[index],
            _ => panic!(),
        }
    }
}
