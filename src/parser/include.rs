use nom::{bytes::complete::tag, IResult};

use crate::ast::include::IncludeDef;
use crate::parser::common::{quoted_string, ws};

/// Parse `include "PATH"`
pub fn parse_include(input: &str) -> IResult<&str, IncludeDef> {
    let (input, _) = tag("include")(input)?;
    let (input, _) = ws(input)?;
    let (input, path) = quoted_string(input)?;
    Ok((
        input,
        IncludeDef {
            path: path.to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_relative_path() {
        let (rest, inc) = parse_include(r#"include "./setup.cvg""#).unwrap();
        assert_eq!(inc.path, "./setup.cvg");
        assert_eq!(rest, "");
    }

    #[test]
    fn include_directory_path() {
        let (rest, inc) = parse_include(r#"include "./clips/drums.cvg""#).unwrap();
        assert_eq!(inc.path, "./clips/drums.cvg");
        assert_eq!(rest, "");
    }
}
