use nom::{bytes::complete::tag, IResult};

use crate::ast::include::IncludeDef;
use crate::parser::common::{path_string, ws};

/// インクルード文をパースする: `include PATH`
/// Parse `include PATH` (unquoted, reads until end of line)
pub fn parse_include(input: &str) -> IResult<&str, IncludeDef> {
    let (input, _) = tag("include")(input)?;
    let (input, _) = ws(input)?;
    let (input, path) = path_string(input)?;
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
        let (rest, inc) = parse_include("include ./setup.cvg\n").unwrap();
        assert_eq!(inc.path, "./setup.cvg");
        assert_eq!(rest, "\n");
    }

    #[test]
    fn include_directory_path() {
        let (rest, inc) = parse_include("include ./clips/drums.cvg\n").unwrap();
        assert_eq!(inc.path, "./clips/drums.cvg");
        assert_eq!(rest, "\n");
    }

    #[test]
    fn include_no_trailing_newline() {
        let (rest, inc) = parse_include("include path/to/deep/file.cvg").unwrap();
        assert_eq!(inc.path, "path/to/deep/file.cvg");
        assert_eq!(rest, "");
    }

    #[test]
    fn include_trims_trailing_spaces() {
        let (rest, inc) = parse_include("include ./setup.cvg   \n").unwrap();
        assert_eq!(inc.path, "./setup.cvg");
        assert_eq!(rest, "\n");
    }
}
