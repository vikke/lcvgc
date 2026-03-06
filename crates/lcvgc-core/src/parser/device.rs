use nom::{bytes::complete::tag, IResult};

use crate::ast::device::DeviceDef;
use crate::parser::common::*;

/// デバイスブロックをパースする: `device NAME { port PORT_STRING }`
/// Parse a device block: `device NAME { port PORT_STRING }`
pub fn parse_device(input: &str) -> IResult<&str, DeviceDef> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("device")(input)?;
    let (input, _) = ws(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("{")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("port")(input)?;
    let (input, _) = ws(input)?;
    let (input, port) = unquoted_value(input)?;
    let (input, _) = tag("}")(input)?;

    Ok((
        input,
        DeviceDef {
            name: name.to_string(),
            port: port.to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_device_basic() {
        let input = "device mutant_brain {\n  port Mutant Brain\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "mutant_brain".to_string(),
                    port: "Mutant Brain".to_string(),
                }
            ))
        );
    }

    #[test]
    fn test_parse_device_space_in_port() {
        let input = "device volca_keys {\n  port volca keys\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "volca_keys".to_string(),
                    port: "volca keys".to_string(),
                }
            ))
        );
    }

    #[test]
    fn test_parse_device_single_word_port() {
        let input = "device mb {\n  port IAC\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "mb".to_string(),
                    port: "IAC".to_string(),
                }
            ))
        );
    }

    #[test]
    fn test_parse_device_multiple_independent() {
        let input1 = "device mutant_brain {\n  port Mutant Brain\n}";
        let input2 = "device volca_keys {\n  port volca keys\n}";

        let result1 = parse_device(input1);
        let result2 = parse_device(input2);

        assert!(result1.is_ok());
        assert!(result2.is_ok());

        assert_eq!(result1.unwrap().1.name, "mutant_brain");
        assert_eq!(result2.unwrap().1.name, "volca_keys");
    }
}
