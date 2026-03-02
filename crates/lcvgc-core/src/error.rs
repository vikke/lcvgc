use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("unexpected token: expected {expected}, found '{found}'")]
    UnexpectedToken { expected: String, found: String },

    #[error("unexpected end of input")]
    UnexpectedEof,

    #[error("invalid note name: '{0}'")]
    InvalidNoteName(String),

    #[error("invalid octave: {0} (must be 0-9)")]
    InvalidOctave(u8),

    #[error("invalid duration: {0}")]
    InvalidDuration(String),

    #[error("invalid identifier: '{0}'")]
    InvalidIdentifier(String),

    #[error("reserved keyword used as identifier: '{0}'")]
    ReservedKeyword(String),

    #[error("parse error: {0}")]
    Nom(String),
}

impl<'a> From<nom::Err<nom::error::Error<&'a str>>> for ParseError {
    fn from(e: nom::Err<nom::error::Error<&'a str>>) -> Self {
        ParseError::Nom(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = ParseError::InvalidNoteName("z".to_string());
        assert_eq!(err.to_string(), "invalid note name: 'z'");
    }

    #[test]
    fn error_from_nom() {
        let nom_err = nom::Err::Error(nom::error::Error::new("test", nom::error::ErrorKind::Tag));
        let err: ParseError = nom_err.into();
        assert!(matches!(err, ParseError::Nom(_)));
    }
}
