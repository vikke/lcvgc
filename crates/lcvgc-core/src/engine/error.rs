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
}
