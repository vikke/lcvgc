#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("不明なデバイス: {0}")]
    UnknownDevice(String),
    #[error("不明なインストゥルメント: {0}")]
    UnknownInstrument(String),
    #[error("不明なキット: {0}")]
    UnknownKit(String),
    #[error("不明なクリップ: {0}")]
    UnknownClip(String),
    #[error("不明なシーン: {0}")]
    UnknownScene(String),
    #[error("不明なセッション: {0}")]
    UnknownSession(String),
    #[error("設定エラー: {0}")]
    Config(String),
    #[error("パースエラー: {0}")]
    ParseError(String),
    #[error("IOエラー: {0}")]
    Io(#[from] std::io::Error),

    /// 循環インクルード検出 / Circular include detected
    #[error("循環インクルード: {0}")]
    CircularInclude(String),

    /// インクルードファイル未検出 / Include file not found
    #[error("インクルードファイル未検出: {0}")]
    IncludeNotFound(String),

    /// インクルードファイル読み込みエラー / Include file read error
    #[error("インクルードファイル読み込みエラー: {path}: {reason}")]
    IncludeReadError {
        /// ファイルパス / File path
        path: String,
        /// エラー原因 / Error reason
        reason: String,
    },

    /// インクルードがファイル先頭にない / Include is not at the top of the file
    #[error("includeはファイル先頭に記述してください: {0}")]
    IncludeNotAtTop(String),

    /// 未定義変数の参照 / Undefined variable reference
    #[error("未定義変数: {name} (フィールド: {field})")]
    UndefinedVariable {
        /// 変数名 / Variable name
        name: String,
        /// 参照元フィールド名 / Referencing field name
        field: String,
    },

    /// コンパイルエラー / Compile error
    #[error("コンパイルエラー: {0}")]
    CompileError(String),

    /// 変数値の型変換失敗 / Variable value type conversion failure
    #[error("変数値の型変換失敗: {name} = \"{value}\" ({expected_type}に変換できません)")]
    InvalidVariableValue {
        /// 変数名 / Variable name
        name: String,
        /// 変数の値 / Variable value
        value: String,
        /// 期待される型の説明 / Expected type description
        expected_type: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_unknown_device() {
        let e = EngineError::UnknownDevice("dev1".into());
        assert_eq!(e.to_string(), "不明なデバイス: dev1");
    }

    #[test]
    fn display_unknown_instrument() {
        let e = EngineError::UnknownInstrument("inst1".into());
        assert_eq!(e.to_string(), "不明なインストゥルメント: inst1");
    }

    #[test]
    fn display_unknown_kit() {
        let e = EngineError::UnknownKit("kit1".into());
        assert_eq!(e.to_string(), "不明なキット: kit1");
    }

    #[test]
    fn display_unknown_clip() {
        let e = EngineError::UnknownClip("clip1".into());
        assert_eq!(e.to_string(), "不明なクリップ: clip1");
    }

    #[test]
    fn display_unknown_scene() {
        let e = EngineError::UnknownScene("scene1".into());
        assert_eq!(e.to_string(), "不明なシーン: scene1");
    }

    #[test]
    fn display_unknown_session() {
        let e = EngineError::UnknownSession("sess1".into());
        assert_eq!(e.to_string(), "不明なセッション: sess1");
    }

    #[test]
    fn display_config() {
        let e = EngineError::Config("bad config".into());
        assert_eq!(e.to_string(), "設定エラー: bad config");
    }

    #[test]
    fn display_io() {
        let e = EngineError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert_eq!(e.to_string(), "IOエラー: not found");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::other("test");
        let e: EngineError = io_err.into();
        assert!(matches!(e, EngineError::Io(_)));
    }

    #[test]
    fn display_circular_include() {
        let e = EngineError::CircularInclude("a.cvg -> b.cvg -> a.cvg".into());
        assert_eq!(e.to_string(), "循環インクルード: a.cvg -> b.cvg -> a.cvg");
    }

    #[test]
    fn display_include_not_found() {
        let e = EngineError::IncludeNotFound("missing.cvg".into());
        assert_eq!(e.to_string(), "インクルードファイル未検出: missing.cvg");
    }

    #[test]
    fn display_include_read_error() {
        let e = EngineError::IncludeReadError {
            path: "broken.cvg".into(),
            reason: "permission denied".into(),
        };
        assert_eq!(
            e.to_string(),
            "インクルードファイル読み込みエラー: broken.cvg: permission denied"
        );
    }

    #[test]
    fn display_undefined_variable() {
        let err = EngineError::UndefinedVariable {
            name: "bass_ch".into(),
            field: "channel".into(),
        };
        assert_eq!(err.to_string(), "未定義変数: bass_ch (フィールド: channel)");
    }

    #[test]
    fn display_compile_error() {
        let e = EngineError::CompileError("bad content".into());
        assert_eq!(e.to_string(), "コンパイルエラー: bad content");
    }

    #[test]
    fn display_invalid_variable_value() {
        let err = EngineError::InvalidVariableValue {
            name: "ch".into(),
            value: "abc".into(),
            expected_type: "u8".into(),
        };
        assert_eq!(
            err.to_string(),
            "変数値の型変換失敗: ch = \"abc\" (u8に変換できません)"
        );
    }
}
