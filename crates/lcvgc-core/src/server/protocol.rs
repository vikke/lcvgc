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
    /// MIDIポート一覧
    #[serde(rename = "list_ports")]
    ListPorts,
}

/// MIDIポート情報
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub direction: String,
}

/// サーバーからのレスポンス
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<PortInfo>>,
}

impl Response {
    /// 成功レスポンス
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            error: None,
            ports: None,
        }
    }

    /// エラーレスポンス
    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            message: None,
            error: Some(error.into()),
            ports: None,
        }
    }

    /// ポート一覧レスポンス
    pub fn ports(ports: Vec<PortInfo>) -> Self {
        Self {
            success: true,
            message: None,
            error: None,
            ports: Some(ports),
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

    #[test]
    fn deserialize_list_ports_request() {
        let json = r#"{"type":"list_ports"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(matches!(req, Request::ListPorts));
    }

    #[test]
    fn serialize_ports_response() {
        let resp = Response::ports(vec![
            PortInfo {
                name: "Synth:0".to_string(),
                direction: "out".to_string(),
            },
            PortInfo {
                name: "Controller:0".to_string(),
                direction: "in".to_string(),
            },
        ]);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"ports\""));
        assert!(json.contains("\"direction\":\"out\""));
        assert!(json.contains("\"direction\":\"in\""));
        assert!(!json.contains("\"message\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn ports_response_content() {
        let ports = vec![PortInfo {
            name: "Test:0".to_string(),
            direction: "out".to_string(),
        }];
        let resp = Response::ports(ports);
        assert!(resp.success);
        assert!(resp.message.is_none());
        assert!(resp.error.is_none());
        let port_list = resp.ports.unwrap();
        assert_eq!(port_list.len(), 1);
        assert_eq!(port_list[0].name, "Test:0");
        assert_eq!(port_list[0].direction, "out");
    }
}
