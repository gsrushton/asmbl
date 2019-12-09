use std::path;

#[derive(Debug, failure::Fail)]
pub enum Error {
    #[fail(display = "File path addresses beneath root.")]
    Underflow,
}

pub struct Relativiser {
    base: path::PathBuf,
}

impl Relativiser {
    pub fn new(base: path::PathBuf) -> Self {
        Self { base }
    }

    pub fn relativise(
        &self,
        context: &Vec<path::Component>,
        path: &path::Path,
    ) -> Result<path::PathBuf, Error> {
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
                        return Err(Error::Underflow);
                    }
                }
                _ => components.push(component),
            }
        }

        let mut shared_component_index: usize = 0;
        while shared_component_index < std::cmp::min(components.len(), context.len())
            && components[shared_component_index] == context[shared_component_index]
        {
            shared_component_index += 1;
        }

        let mut path = path::PathBuf::new();

        // Walk backwards until we match with context..
        for _ in shared_component_index..context.len() {
            path.push("..");
        }

        // Walk forwards building the rest of the path...
        for c in shared_component_index..components.len() {
            path.push(components[c]);
        }

        Ok(path)
    }
}
