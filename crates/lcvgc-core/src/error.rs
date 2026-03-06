use thiserror::Error;

/// DSLパースエラー
/// DSL parse error
#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    /// 予期しないトークン
    /// Unexpected token encountered
    #[error("unexpected token: expected {expected}, found '{found}'")]
    UnexpectedToken {
        /// 期待されたトークン / Expected token
        expected: String,
        /// 実際に見つかったトークン / Token actually found
        found: String,
    },

    /// 入力の予期しない終端
    /// Unexpected end of input
    #[error("unexpected end of input")]
    UnexpectedEof,

    /// 無効なノート名
    /// Invalid note name
    #[error("invalid note name: '{0}'")]
    InvalidNoteName(String),

    /// 無効なオクターブ（0-9の範囲外）
    /// Invalid octave (outside 0-9 range)
    #[error("invalid octave: {0} (must be 0-9)")]
    InvalidOctave(u8),

    /// 無効な音価
    /// Invalid duration
    #[error("invalid duration: {0}")]
    InvalidDuration(String),

    /// 無効な識別子
    /// Invalid identifier
    #[error("invalid identifier: '{0}'")]
    InvalidIdentifier(String),

    /// 予約語が識別子として使用された
    /// Reserved keyword used as identifier
    #[error("reserved keyword used as identifier: '{0}'")]
    ReservedKeyword(String),

    /// nomパーサーからのエラー
    /// Error from the nom parser
    #[error("parse error: {0}")]
    Nom(String),
}

/// nomエラーからParseErrorへの変換
/// Conversion from nom error to ParseError
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
