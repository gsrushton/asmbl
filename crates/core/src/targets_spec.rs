use std::path;

#[derive(Clone)]
pub struct TargetSpec {
    path: String,
}

#[derive(Debug, failure::Fail)]
pub enum ResolveError {
    #[fail(display = "Invalid marker character.")]
    InvalidMarkerCharacter,
    #[fail(display = "Missing marker character.")]
    MissingMarkerCharacter,
    #[fail(display = "No input")]
    NoInput,
    #[fail(display = "No file-stem")]
    NoFileStem,
    #[fail(display = "Non unicode input path")]
    NonUnicodeInputPath,
}

impl TargetSpec {
    pub fn resolve(
        self,
        mut prefix: path::PathBuf,
        input: Option<&path::Path>,
    ) -> Result<path::PathBuf, ResolveError> {
        let mut path = String::with_capacity(
            self.path.len() + input.map(|i| i.as_os_str().len()).unwrap_or(0),
        );

        let mut it = self.path.chars();
        while let Some(ch) = it.next() {
            if ch == '%' {
                match it.next() {
                    Some('%') => path.push('%'),
                    Some('f') => path.push_str(
                        input
                            .ok_or(ResolveError::NoInput)?
                            .file_stem()
                            .ok_or(ResolveError::NoFileStem)?
                            .to_str()
                            .ok_or(ResolveError::NonUnicodeInputPath)?,
                    ),
                    None => return Err(ResolveError::MissingMarkerCharacter),
                    _ => return Err(ResolveError::InvalidMarkerCharacter),
                }
            } else {
                path.push(ch);
            }
        }

        prefix.push(path::Path::new(&path));

        Ok(prefix)
    }
}

impl From<String> for TargetSpec {
    fn from(path: String) -> Self {
        // TODO would be nice to check markers here...

        Self { path }
    }
}

#[derive(Clone)]
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
}

impl From<Vec<String>> for TargetsSpec {
    fn from(mut paths: Vec<String>) -> Self {
        if paths.len() == 1 {
            Self::Single(TargetSpec::from(paths.pop().unwrap()))
        } else {
            Self::Multi(
                paths
                    .into_iter()
                    .map(|path| TargetSpec::from(path))
                    .collect(),
            )
        }
    }
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
