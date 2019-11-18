use super::{ArgElement, Variable};
use nom::*;

fn space(c: char) -> bool {
    (c == ' ') | (c == '\t') | (c == '\n')
}

fn numeric(c: char) -> bool {
    match c {
        '0'..='9' => true,
        _ => false,
    }
}

fn alphanumeric(c: char) -> bool {
    match c {
        '0'..='9' | 'a'..='z' | 'A'..='Z' | '_' | '-' => true,
        _ => false,
    }
}

fn index(i: &str) -> IResult<&str, usize> {
    nom::sequence::delimited(
        nom::character::complete::char('['),
        nom::combinator::map_res(nom::bytes::complete::take_while1(|c| numeric(c)), |s| {
            usize::from_str_radix(s, 10)
        }),
        nom::character::complete::char(']'),
    )(i)
}

fn variable_name(i: &str) -> IResult<&str, &str> {
    nom::branch::alt((
        nom::bytes::complete::tag("<"),
        nom::bytes::complete::tag("@"),
        nom::bytes::complete::take_while1(|c| alphanumeric(c)),
    ))(i)
}

fn variable(i: &str) -> IResult<&str, Variable> {
    let (r, (_, name, index)) = nom::sequence::tuple((
        nom::character::complete::char('$'),
        variable_name,
        nom::combinator::opt(index),
    ))(i)?;

    match name {
        "@" | "targets" => match index {
            Some(index) => Ok((r, Variable::Target(index))),
            None => Ok((r, Variable::Targets)),
        },
        "<" | "inputs" => match index {
            Some(index) => Ok((r, Variable::Input(index))),
            None => Ok((r, Variable::Inputs)),
        },
        _ => Ok((r, Variable::Other(name.to_string()))),
    }
}

fn element(i: &str) -> IResult<&str, ArgElement> {
    nom::branch::alt((
        nom::combinator::map(variable, |v| ArgElement::Var(v)),
        nom::combinator::map(
            nom::bytes::complete::take_while1(|c| c != '$'),
            |s: &str| ArgElement::Str(s.to_string()),
        ),
    ))(i)
}

fn elements(i: &str) -> IResult<&str, Vec<ArgElement>> {
    nom::multi::many1(element)(i)
}

#[derive(Debug, failure::Fail)]
#[fail(display = "Error parsing elements from argument")]
pub struct ParseElementsError;

pub fn parse_elements(i: &str) -> Result<Vec<ArgElement>, ParseElementsError> {
    match elements(i) {
        Ok((_, elements)) => Ok(elements),
        Err(_) => Err(ParseElementsError {}),
    }
}

fn escaped_str(i: &str) -> IResult<&str, String> {
    nom::bytes::complete::escaped_transform(
        nom::character::complete::none_of(r#"\""#),
        '\\',
        nom::branch::alt((
            nom::combinator::map(nom::bytes::complete::tag("\\"), |_| "\\"),
            nom::combinator::map(nom::bytes::complete::tag("\""), |_| "\""),
            nom::combinator::map(nom::bytes::complete::tag("\n"), |_| "\n"),
        )),
    )(i)
}

fn args(i: &str) -> IResult<&str, Vec<String>> {
    nom::multi::many0(nom::sequence::delimited(
        nom::character::complete::space0,
        nom::branch::alt((
            nom::sequence::delimited(
                nom::character::complete::char('"'),
                escaped_str,
                nom::character::complete::char('"'),
            ),
            nom::sequence::terminated(
                nom::combinator::map(nom::bytes::complete::take_till1(|c| space(c)), |s: &str| {
                    s.to_string()
                }),
                nom::character::complete::space0,
            ),
        )),
        nom::character::complete::space0,
    ))(i)
}

#[derive(Debug, failure::Fail)]
#[fail(display = "Error parsing arguments from string")]
pub struct ParseArgsError;

pub fn parse_args(i: &str) -> Result<Vec<String>, ParseArgsError> {
    match args(i) {
        Ok((_, args)) => Ok(args),
        Err(_) => Err(ParseArgsError {}),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_parse_index() {
        assert_eq!(index("[0]"), Ok(("", 0)));
        assert_eq!(index("[13]"), Ok(("", 13)));
        assert_eq!(index("[65472]"), Ok(("", 65472)));
    }

    #[test]
    fn can_parse_variable_name() {
        assert_eq!(variable_name("<"), Ok(("", "<")));
        assert_eq!(variable_name("@"), Ok(("", "@")));
        assert_eq!(variable_name("cake"), Ok(("", "cake")));
        assert_eq!(variable_name("cheese_cake"), Ok(("", "cheese_cake")));
    }

    #[test]
    fn can_parse_variable() {
        assert_eq!(variable("$<"), Ok(("", Variable::Inputs)));
        assert_eq!(variable("$<[7]"), Ok(("", Variable::Input(7))));
        assert_eq!(variable("$@"), Ok(("", Variable::Targets)));
        assert_eq!(variable("$@[29]"), Ok(("", Variable::Target(29))));
        assert_eq!(
            variable("$cake"),
            Ok(("", Variable::Other("cake".to_string())))
        );
        assert_eq!(
            variable("$cheese_cake[111]"),
            Ok(("", Variable::Other("cheese_cake".to_string())))
        );
    }

    #[test]
    fn can_parse_element() {
        assert_eq!(
            element("argument"),
            Ok(("", ArgElement::Str("argument".to_string())))
        );
        assert_eq!(element("$<"), Ok(("", ArgElement::Var(Variable::Inputs))));
        assert_eq!(
            element("$<[7]"),
            Ok(("", ArgElement::Var(Variable::Input(7))))
        );
        assert_eq!(element("$@"), Ok(("", ArgElement::Var(Variable::Targets))));
        assert_eq!(
            element("$@[29]"),
            Ok(("", ArgElement::Var(Variable::Target(29))))
        );
        assert_eq!(
            element("$cake"),
            Ok(("", ArgElement::Var(Variable::Other("cake".to_string()))))
        );
        assert_eq!(
            element("$cheese_cake[111]"),
            Ok((
                "",
                ArgElement::Var(Variable::Other("cheese_cake".to_string()))
            ))
        );
        assert_eq!(
            element("cake $<"),
            Ok(("$<", ArgElement::Str("cake ".to_string())))
        );
        assert_eq!(
            element("$@[42] cheese"),
            Ok((" cheese", ArgElement::Var(Variable::Target(42))))
        );
    }

    #[test]
    fn can_parse_elements() {
        assert_eq!(
            elements("argument"),
            Ok(("", vec![ArgElement::Str("argument".to_string())]))
        );
    }

    #[test]
    fn can_parse_args() {
        assert_eq!(
            args("some simple args"),
            Ok((
                "",
                vec!["some".to_string(), "simple".to_string(), "args".to_string()]
            ))
        );

        assert_eq!(
            args(r#"some "quoted with spaces" args"#),
            Ok((
                "",
                vec![
                    "some".to_string(),
                    "quoted with spaces".to_string(),
                    "args".to_string()
                ]
            ))
        );

        assert_eq!(
            args(r#"some "quoted with spaces and \"escaped\" quotes" args"#),
            Ok((
                "",
                vec![
                    "some".to_string(),
                    r#"quoted with spaces and "escaped" quotes"#.to_string(),
                    "args".to_string()
                ]
            ))
        );

        assert_eq!(
            args(r#""two quoted" "strings back to back""#),
            Ok((
                "",
                vec!["two quoted".to_string(), "strings back to back".to_string()]
            ))
        );
    }
}
