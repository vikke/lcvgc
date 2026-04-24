use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    combinator::map,
    IResult,
};

use crate::ast::device::DeviceDef;
use crate::parser::common::*;

/// `port <VALUE>` 行の値（行末または `}` まで）を取り出す。
/// port の値は引用符なしで、改行か `}` で終端する。
/// Reads the value of a `port <VALUE>` line (until newline or `}`).
/// The value is unquoted and terminated by a newline or `}`.
fn port_line_value(input: &str) -> IResult<&str, String> {
    let (remaining, taken) = take_while(|c: char| c != '\n' && c != '}')(input)?;
    let trimmed = taken.trim();
    if trimmed.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TakeWhile1,
        )));
    }
    Ok((remaining, trimmed.to_string()))
}

/// `transport <true|false>` 行をパースする。
/// Parse a `transport <true|false>` line.
fn parse_transport_value(input: &str) -> IResult<&str, bool> {
    alt((map(tag("true"), |_| true), map(tag("false"), |_| false)))(input)
}

/// device ブロック内の単一行（`port ...` または `transport ...`）をパースする。
/// `port` / `transport` の**順序は任意**で、いずれも最大 1 回まで指定できる。
/// Parse a single entry (either `port` or `transport`) inside a device block.
/// Order is arbitrary; each key may appear at most once.
enum DeviceEntry {
    Port(String),
    Transport(bool),
}

fn parse_device_entry(input: &str) -> IResult<&str, DeviceEntry> {
    alt((
        // transport true|false
        |i| {
            let (i, _) = tag("transport")(i)?;
            let (i, _) = take_while1(|c: char| c == ' ' || c == '\t')(i)?;
            let (i, v) = parse_transport_value(i)?;
            Ok((i, DeviceEntry::Transport(v)))
        },
        // port <value>
        |i| {
            let (i, _) = tag("port")(i)?;
            let (i, _) = take_while1(|c: char| c == ' ' || c == '\t')(i)?;
            let (i, v) = port_line_value(i)?;
            Ok((i, DeviceEntry::Port(v)))
        },
    ))(input)
}

/// デバイスブロックをパースする: `device NAME { port PORT_STRING [transport BOOL] }`
/// Parse a device block: `device NAME { port PORT_STRING [transport BOOL] }`
///
/// * `port` は必須。
/// * `transport` は省略可（省略時 `true`）。`true` または `false` のいずれか。
/// * `port` の値は改行または `}` で終端し、前後の空白は trim される。
///
/// * `port` is required.
/// * `transport` is optional and defaults to `true`; must be `true` or `false`.
/// * The `port` value terminates at a newline or `}`, with surrounding whitespace trimmed.
pub fn parse_device(input: &str) -> IResult<&str, DeviceDef> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("device")(input)?;
    let (input, _) = ws(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("{")(input)?;
    let (input, _) = ws(input)?;

    let mut input_cursor = input;
    let mut port: Option<String> = None;
    let mut transport: Option<bool> = None;

    loop {
        // `}` に遭遇したらブロック終端
        if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("}")(input_cursor) {
            input_cursor = rest;
            break;
        }
        let (next, entry) = parse_device_entry(input_cursor)?;
        match entry {
            DeviceEntry::Port(v) => {
                if port.is_some() {
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input_cursor,
                        nom::error::ErrorKind::Many1,
                    )));
                }
                port = Some(v);
            }
            DeviceEntry::Transport(b) => {
                if transport.is_some() {
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input_cursor,
                        nom::error::ErrorKind::Many1,
                    )));
                }
                transport = Some(b);
            }
        }
        let (next, _) = ws(next)?;
        input_cursor = next;
    }

    let port = port.ok_or_else(|| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
    })?;

    Ok((
        input_cursor,
        DeviceDef {
            name: name.to_string(),
            port,
            transport: transport.unwrap_or(true),
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
                    transport: true,
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
                    transport: true,
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
                    transport: true,
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

    #[test]
    fn test_parse_device_transport_true() {
        let input = "device mb {\n  port IAC\n  transport true\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "mb".to_string(),
                    port: "IAC".to_string(),
                    transport: true,
                }
            ))
        );
    }

    #[test]
    fn test_parse_device_transport_false() {
        let input = "device mb {\n  port IAC\n  transport false\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "mb".to_string(),
                    port: "IAC".to_string(),
                    transport: false,
                }
            ))
        );
    }

    #[test]
    fn test_parse_device_transport_before_port() {
        let input = "device mb {\n  transport false\n  port IAC\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "mb".to_string(),
                    port: "IAC".to_string(),
                    transport: false,
                }
            ))
        );
    }

    #[test]
    fn test_parse_device_transport_invalid_value() {
        let input = "device mb {\n  port IAC\n  transport yes\n}";
        let result = parse_device(input);
        assert!(result.is_err(), "invalid transport value must fail");
    }

    #[test]
    fn test_parse_device_missing_port() {
        let input = "device mb {\n  transport true\n}";
        let result = parse_device(input);
        assert!(result.is_err(), "missing port must fail");
    }

    #[test]
    fn test_parse_device_space_port_then_transport() {
        let input = "device mb {\n  port Mutant Brain\n  transport false\n}";
        let result = parse_device(input);
        assert_eq!(
            result,
            Ok((
                "",
                DeviceDef {
                    name: "mb".to_string(),
                    port: "Mutant Brain".to_string(),
                    transport: false,
                }
            ))
        );
    }
}
