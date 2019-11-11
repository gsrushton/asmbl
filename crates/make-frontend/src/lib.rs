use std::{fs, path};

use asmbl_core as core;
use asmbl_utils as utils;

mod parser;

pub struct FrontEnd {}

impl FrontEnd {
    pub fn new() -> Self {
        Self {}
    }
}

impl core::FrontEnd for FrontEnd {
    fn parse_unit<'v, 'p>(
        &self,
        path: &path::Path,
        mut unit_builder: core::UnitBuilder<'v, 'p>,
    ) -> Result<core::Unit, core::ParseUnitError> {
        let content = utils::io::read_file(fs::File::open(path)?)?;

        let rules = parser::parse(&content)
            .map_err(|err| core::ParseUnitError::Other(failure::Error::from(err)))?;

        for rule in rules {
            for target in rule.targets {
                for prerequisite in rule.prerequisites.iter() {
                    unit_builder.add_prerequisite(
                        path::Path::new(&target),
                        path::Path::new(&prerequisite),
                    )?
                }
            }
        }

        Ok(unit_builder.unit())
    }
}
