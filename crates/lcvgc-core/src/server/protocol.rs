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
    /// LSP補完リクエスト
    #[serde(rename = "lsp_completion")]
    LspCompletion { source: String, offset: usize },
    /// LSPホバーリクエスト
    #[serde(rename = "lsp_hover")]
    LspHover { source: String, offset: usize },
    /// LSP診断リクエスト
    #[serde(rename = "lsp_diagnostics")]
    LspDiagnostics { source: String },
    /// LSP定義ジャンプリクエスト
    #[serde(rename = "lsp_goto_definition")]
    LspGotoDefinition { source: String, offset: usize },
    /// LSPドキュメントシンボルリクエスト
    #[serde(rename = "lsp_document_symbols")]
    LspDocumentSymbols { source: String },
}

/// MIDIポート情報
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub direction: String,
}

/// LSP補完アイテム
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspCompletionItem {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: String,
}

/// LSPホバー情報
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspHoverInfo {
    pub content: String,
}

/// LSP診断アイテム
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspDiagnosticItem {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub message: String,
    pub severity: String,
}

/// LSP位置スパン
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspLocationSpan {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// LSPシンボルアイテム
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspSymbolItem {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// LSP結果
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum LspResult {
    #[serde(rename = "completion")]
    Completion { items: Vec<LspCompletionItem> },
    #[serde(rename = "hover")]
    Hover { info: Option<LspHoverInfo> },
    #[serde(rename = "diagnostics")]
    Diagnostics { items: Vec<LspDiagnosticItem> },
    #[serde(rename = "goto_definition")]
    GotoDefinition { location: Option<LspLocationSpan> },
    #[serde(rename = "document_symbols")]
    DocumentSymbols { items: Vec<LspSymbolItem> },
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
    /// LSP結果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsp: Option<LspResult>,
}

impl Response {
    /// 成功レスポンス
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            error: None,
            ports: None,
            lsp: None,
        }
    }

    /// エラーレスポンス
    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            message: None,
            error: Some(error.into()),
            ports: None,
            lsp: None,
        }
    }

    /// ポート一覧レスポンス
    pub fn ports(ports: Vec<PortInfo>) -> Self {
        Self {
            success: true,
            message: None,
            error: None,
            ports: Some(ports),
            lsp: None,
        }
    }

    /// LSP結果レスポンス
    pub fn lsp(result: LspResult) -> Self {
        Self {
            success: true,
            message: None,
            error: None,
            ports: None,
            lsp: Some(result),
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
    fn response_ok_has_no_lsp() {
        let resp = Response::ok("hello");
        assert!(resp.lsp.is_none());
    }

    #[test]
    fn response_err_has_no_lsp() {
        let resp = Response::err("fail");
        assert!(resp.lsp.is_none());
    }

    #[test]
    fn deserialize_lsp_completion_request() {
        let json = r#"{"type":"lsp_completion","source":"tempo 120","offset":5}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::LspCompletion { source, offset } => {
                assert_eq!(source, "tempo 120");
                assert_eq!(offset, 5);
            }
            _ => panic!("Expected LspCompletion"),
        }
    }

    #[test]
    fn deserialize_lsp_hover_request() {
        let json = r#"{"type":"lsp_hover","source":"tempo 120","offset":3}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::LspHover { source, offset } => {
                assert_eq!(source, "tempo 120");
                assert_eq!(offset, 3);
            }
            _ => panic!("Expected LspHover"),
        }
    }

    #[test]
    fn deserialize_lsp_diagnostics_request() {
        let json = r#"{"type":"lsp_diagnostics","source":"tempo 120"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::LspDiagnostics { source } => assert_eq!(source, "tempo 120"),
            _ => panic!("Expected LspDiagnostics"),
        }
    }

    #[test]
    fn deserialize_lsp_goto_definition_request() {
        let json = r#"{"type":"lsp_goto_definition","source":"tempo 120","offset":0}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::LspGotoDefinition { source, offset } => {
                assert_eq!(source, "tempo 120");
                assert_eq!(offset, 0);
            }
            _ => panic!("Expected LspGotoDefinition"),
        }
    }

    #[test]
    fn deserialize_lsp_document_symbols_request() {
        let json = r#"{"type":"lsp_document_symbols","source":"tempo 120"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::LspDocumentSymbols { source } => assert_eq!(source, "tempo 120"),
            _ => panic!("Expected LspDocumentSymbols"),
        }
    }

    #[test]
    fn serialize_lsp_completion_response() {
        let result = LspResult::Completion {
            items: vec![LspCompletionItem {
                label: "tempo".to_string(),
                detail: None,
                kind: "Keyword".to_string(),
            }],
        };
        let resp = Response::lsp(result);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"lsp\""));
        assert!(json.contains("\"tempo\""));
    }

    #[test]
    fn lsp_response_fields() {
        let result = LspResult::Hover { info: None };
        let resp = Response::lsp(result);
        assert!(resp.success);
        assert!(resp.message.is_none());
        assert!(resp.error.is_none());
        assert!(resp.ports.is_none());
        assert!(resp.lsp.is_some());
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
