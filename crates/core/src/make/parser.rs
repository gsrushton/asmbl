use nom;

fn space(i: &str) -> nom::IResult<&str, &str> {
    nom::branch::alt((
        nom::bytes::complete::tag(" "),
        nom::bytes::complete::tag("\t"),
        nom::bytes::complete::tag("\\\n"),
    ))(i)
}

fn space0(i: &str) -> nom::IResult<&str, Vec<&str>> {
    nom::multi::many0(space)(i)
}

fn space1(i: &str) -> nom::IResult<&str, Vec<&str>> {
    nom::multi::many1(space)(i)
}

fn ident(i: &str) -> nom::IResult<&str, &str> {
    nom::bytes::complete::is_not(" \t\\\n:")(i)
}

fn idents(i: &str) -> nom::IResult<&str, Vec<&str>> {
    nom::sequence::delimited(
        space0,
        nom::multi::separated_nonempty_list(space1, ident),
        space0,
    )(i)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Rule<'a> {
    pub targets: Vec<&'a str>,
    pub prerequisites: Vec<&'a str>,
}

fn rule(i: &str) -> nom::IResult<&str, Rule> {
    nom::sequence::delimited(
        nom::character::complete::multispace0,
        nom::combinator::map(
            nom::sequence::tuple((
                idents,
                nom::bytes::complete::is_a(":"),
                nom::combinator::opt(idents),
                nom::combinator::opt(nom::character::complete::newline),
            )),
            |(targets, _, prerequisites, _)| Rule {
                targets,
                prerequisites: prerequisites.unwrap_or_else(|| vec![]),
            },
        ),
        nom::character::complete::multispace0,
    )(i)
}

fn rules(i: &str) -> nom::IResult<&str, Vec<Rule>> {
    nom::multi::many0(rule)(i)
}

#[derive(Debug, failure::Fail)]
pub enum Error {
    #[fail(display = "Incomplete")]
    Incomplete,
    #[fail(display = "{}", 0)]
    Failure(String),
}

pub fn parse(i: &str) -> Result<Vec<Rule>, Error> {
    match rules(i) {
        Ok((_, rules)) => Ok(rules),
        Err(err) => Err(match err {
            nom::Err::Incomplete(_) => Error::Incomplete,
            nom::Err::Error((_, kind)) | nom::Err::Failure((_, kind)) => {
                Error::Failure(kind.description().to_string())
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_space() {
        assert_eq!(space(" "), Ok(("", " ")));
        assert_eq!(space("\t"), Ok(("", "\t")));
        assert_eq!(space("\\\n"), Ok(("", "\\\n")));
    }

    #[test]
    fn can_parse_space0() {
        assert_eq!(space0(""), Ok(("", vec![])));
        assert_eq!(space0(" \t"), Ok(("", vec![" ", "\t"])));
        assert_eq!(space0("\t\\\n"), Ok(("", vec!["\t", "\\\n"])));
        assert_eq!(space0("\\\n "), Ok(("", vec!["\\\n", " "])));
        assert_eq!(space0("x"), Ok(("x", vec![])));
    }

    #[test]
    fn can_parse_ident() {
        assert_eq!(ident("3"), Ok(("", "3")));
        assert_eq!(ident("main.cake"), Ok(("", "main.cake")));
        assert_eq!(ident("abc/main.cake"), Ok(("", "abc/main.cake")));
        assert_eq!(ident("abc/main.cake "), Ok((" ", "abc/main.cake")));
    }

    #[test]
    fn can_parse_idents() {
        assert_eq!(idents("3"), Ok(("", vec!["3"])));
        assert_eq!(idents("\t3 "), Ok(("", vec!["3"])));
        assert_eq!(idents("a b\tc"), Ok(("", vec!["a", "b", "c"])));
        assert_eq!(idents(" a b\tc\t"), Ok(("", vec!["a", "b", "c"])));
        assert_eq!(idents("\\\na\\\nb\\\nc\\\n"), Ok(("", vec!["a", "b", "c"])));
        assert_eq!(idents("ident:"), Ok((":", vec!["ident"])));
        assert_eq!(idents(" ident\t:"), Ok((":", vec!["ident"])));
        assert_eq!(idents("\tident :"), Ok((":", vec!["ident"])));
    }

    #[test]
    fn can_parse_rule() {
        assert_eq!(
            rule("target: prerequisite"),
            Ok((
                "",
                Rule {
                    targets: vec!["target"],
                    prerequisites: vec!["prerequisite"]
                }
            ))
        );

        assert_eq!(
            rule("a b c: d e  f"),
            Ok((
                "",
                Rule {
                    targets: vec!["a", "b", "c"],
                    prerequisites: vec!["d", "e", "f"]
                }
            ))
        );

        assert_eq!(
            rule("a\tb\tc:"),
            Ok((
                "",
                Rule {
                    targets: vec!["a", "b", "c"],
                    prerequisites: vec![]
                }
            ))
        );

        assert_eq!(
            rule("a\\\nb\\\n:\\\nd\\\n"),
            Ok((
                "",
                Rule {
                    targets: vec!["a", "b"],
                    prerequisites: vec!["d"]
                }
            ))
        );
    }

    #[test]
    fn can_parse_rules() {
        assert_eq!(
            rules(
                r"a b: c d
                  x y: z"
            ),
            Ok((
                "",
                vec![
                    Rule {
                        targets: vec!["a", "b"],
                        prerequisites: vec!["c", "d"]
                    },
                    Rule {
                        targets: vec!["x", "y"],
                        prerequisites: vec!["z"]
                    }
                ]
            ))
        );

        assert_eq!(
            rules(
                r"a b: c d

                  x y: z"
            ),
            Ok((
                "",
                vec![
                    Rule {
                        targets: vec!["a", "b"],
                        prerequisites: vec!["c", "d"]
                    },
                    Rule {
                        targets: vec!["x", "y"],
                        prerequisites: vec!["z"]
                    }
                ]
            ))
        );
    }
}
