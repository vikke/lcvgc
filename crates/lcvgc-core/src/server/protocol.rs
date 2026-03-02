use serde::{Deserialize, Serialize};

/// クライアントからのリクエスト
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    /// DSLソースを評価
    #[serde(rename = "eval")]
    Eval { source: String },
    /// ファイルを読み込んで評価
    #[serde(rename = "load")]
    Load { path: String },
    /// ステータス問い合わせ
    #[serde(rename = "status")]
    Status,
}

/// サーバーからのレスポンス
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    /// 成功レスポンス
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            error: None,
        }
    }

    /// エラーレスポンス
    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            message: None,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_eval_request() {
        let json = r#"{"type":"eval","source":"tempo 140"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::Eval { source } => assert_eq!(source, "tempo 140"),
            _ => panic!("Expected Eval"),
        }
    }

    #[test]
    fn deserialize_load_request() {
        let json = r#"{"type":"load","path":"/tmp/test.cvg"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::Load { path } => assert_eq!(path, "/tmp/test.cvg"),
            _ => panic!("Expected Load"),
        }
    }

    #[test]
    fn deserialize_status_request() {
        let json = r#"{"type":"status"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(matches!(req, Request::Status));
    }

    #[test]
    fn deserialize_unknown_type_fails() {
        let json = r#"{"type":"unknown"}"#;
        let result = serde_json::from_str::<Request>(json);
        assert!(result.is_err());
    }

    #[test]
    fn serialize_ok_response() {
        let resp = Response::ok("done");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"message\":\"done\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn serialize_err_response() {
        let resp = Response::err("bad input");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"error\":\"bad input\""));
        assert!(!json.contains("\"message\""));
    }

    #[test]
    fn response_ok_message_content() {
        let resp = Response::ok("hello");
        assert!(resp.success);
        assert_eq!(resp.message.as_deref(), Some("hello"));
        assert!(resp.error.is_none());
    }

    #[test]
    fn response_err_error_content() {
        let resp = Response::err("fail");
        assert!(!resp.success);
        assert!(resp.message.is_none());
        assert_eq!(resp.error.as_deref(), Some("fail"));
    }
}
