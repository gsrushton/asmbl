mod parser;

pub use parser::Error as ParserError;

pub struct Iterator<'a> {
  rules: Vec<parser::Rule<'a>>,
  rule_index: usize,
  target_index: usize,
  prerequisites_index: usize
}

impl<'a> Iterator<'a> {
  fn new(rules: Vec<parser::Rule<'a>>) -> Self {
      Self {
        rules,
        rule_index: 0,
        target_index: 0,
        prerequisites_index: 0
      }
  }
}

impl<'a> std::iter::Iterator for Iterator<'a> {
  type Item = (&'a str, &'a str);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      if self.rule_index >= self.rules.len() {
        return None;
      }

      loop {
        let rule = &self.rules[self.rule_index];
        if self.target_index >= rule.targets.len() {
          self.target_index = 0;
          self.rule_index += 1;
          break;
        }

        let target = rule.targets[self.target_index];
        if self.prerequisites_index >= rule.prerequisites.len() {
          self.prerequisites_index = 0;
          self.target_index += 1;
        } else {
          let prerequisite = rule.prerequisites[self.prerequisites_index];
          self.prerequisites_index += 1;
          return Some((target, prerequisite));
        }
      }
    }
  }
}

pub fn cake(i: &str) -> Result<Iterator, parser::Error> {
    Ok(Iterator::new(parser::parse(i)?))
}
