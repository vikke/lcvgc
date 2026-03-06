use nom::{
    bytes::complete::{tag, take_while1},
    character::complete::char,
    IResult,
};

use crate::ast::var::VarDef;
use crate::parser::common::{non_reserved_identifier, ws};

/// 変数定義をパースする: `var NAME = VALUE`
/// VALUEは次の空白文字または入力末尾まで消費される。
/// Parse `var NAME = VALUE`
/// VALUE is consumed until the next whitespace or end of input.
pub fn parse_var(input: &str) -> IResult<&str, VarDef> {
    let (input, _) = tag("var")(input)?;
    let (input, _) = ws(input)?;
    let (input, name) = non_reserved_identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('=')(input)?;
    let (input, _) = ws(input)?;
    let (input, value) = take_while1(|c: char| !c.is_whitespace())(input)?;
    Ok((
        input,
        VarDef {
            name: name.to_string(),
            value: value.to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_identifier_value() {
        let (rest, var) = parse_var("var dev = mutant_brain").unwrap();
        assert_eq!(var.name, "dev");
        assert_eq!(var.value, "mutant_brain");
        assert_eq!(rest, "");
    }

    #[test]
    fn var_numeric_value() {
        let (rest, var) = parse_var("var gate = 80").unwrap();
        assert_eq!(var.name, "gate");
        assert_eq!(var.value, "80");
        assert_eq!(rest, "");
    }

    #[test]
    fn var_reserved_word_as_name_fails() {
        assert!(parse_var("var var = something").is_err());
        assert!(parse_var("var include = something").is_err());
        assert!(parse_var("var device = something").is_err());
    }
}
