

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

    pub fn name(&self) -> &str {
      &self.name
    }

    pub fn value(&self) -> &EnvSpecValue {
      &self.value
    }
}
