use serde::{Deserialize, Serialize};

/// クライアントからのリクエスト
/// Request from a client
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    /// DSLソースを評価
    /// Evaluate DSL source code
    #[serde(rename = "eval")]
    Eval {
        /// 評価するDSLソース / DSL source to evaluate
        source: String,
    },
    /// ファイルを読み込んで評価
    /// Load and evaluate a file
    #[serde(rename = "load")]
    Load {
        /// ファイルパス / File path
        path: String,
    },
    /// ステータス問い合わせ
    /// Query current status
    #[serde(rename = "status")]
    Status,
    /// MIDIポート一覧
    /// List MIDI ports
    #[serde(rename = "list_ports")]
    ListPorts,
    /// LSP補完リクエスト
    /// LSP completion request
    #[serde(rename = "lsp_completion")]
    LspCompletion {
        /// DSLソース / DSL source
        source: String,
        /// カーソル位置（バイトオフセット） / Cursor position (byte offset)
        offset: usize,
    },
    /// LSPホバーリクエスト
    /// LSP hover request
    #[serde(rename = "lsp_hover")]
    LspHover {
        /// DSLソース / DSL source
        source: String,
        /// カーソル位置（バイトオフセット） / Cursor position (byte offset)
        offset: usize,
    },
    /// LSP診断リクエスト
    /// LSP diagnostics request
    #[serde(rename = "lsp_diagnostics")]
    LspDiagnostics {
        /// DSLソース / DSL source
        source: String,
    },
    /// LSP定義ジャンプリクエスト
    /// LSP go-to-definition request
    #[serde(rename = "lsp_goto_definition")]
    LspGotoDefinition {
        /// DSLソース / DSL source
        source: String,
        /// カーソル位置（バイトオフセット） / Cursor position (byte offset)
        offset: usize,
    },
    /// LSPドキュメントシンボルリクエスト
    /// LSP document symbols request
    #[serde(rename = "lsp_document_symbols")]
    LspDocumentSymbols {
        /// DSLソース / DSL source
        source: String,
    },
}

/// MIDIポート情報
/// MIDI port information
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PortInfo {
    /// ポート名 / Port name
    pub name: String,
    /// 方向 ("in" または "out") / Direction ("in" or "out")
    pub direction: String,
}

/// LSP補完アイテム
/// LSP completion item
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspCompletionItem {
    /// 補完ラベル / Completion label
    pub label: String,
    /// 詳細情報 / Detail information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 補完種別 (e.g. "Keyword", "Snippet") / Completion kind
    pub kind: String,
}

/// LSPホバー情報
/// LSP hover information
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspHoverInfo {
    /// ホバー表示用コンテンツ / Content for hover display
    pub content: String,
}

/// LSP診断アイテム
/// LSP diagnostic item
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspDiagnosticItem {
    /// 開始行 / Start line
    pub start_line: u32,
    /// 開始列 / Start column
    pub start_col: u32,
    /// 終了行 / End line
    pub end_line: u32,
    /// 終了列 / End column
    pub end_col: u32,
    /// 診断メッセージ / Diagnostic message
    pub message: String,
    /// 重要度 (e.g. "Error", "Warning") / Severity level
    pub severity: String,
}

/// LSP位置スパン
/// LSP location span
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspLocationSpan {
    /// 開始行 / Start line
    pub start_line: u32,
    /// 開始列 / Start column
    pub start_col: u32,
    /// 終了行 / End line
    pub end_line: u32,
    /// 終了列 / End column
    pub end_col: u32,
}

/// LSPシンボルアイテム
/// LSP symbol item
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LspSymbolItem {
    /// シンボル名 / Symbol name
    pub name: String,
    /// シンボル種別 (e.g. "Tempo", "Device") / Symbol kind
    pub kind: String,
    /// 開始行 / Start line
    pub start_line: u32,
    /// 開始列 / Start column
    pub start_col: u32,
    /// 終了行 / End line
    pub end_line: u32,
    /// 終了列 / End column
    pub end_col: u32,
}

/// LSP結果
/// LSP result
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum LspResult {
    /// 補完結果 / Completion result
    #[serde(rename = "completion")]
    Completion {
        /// 補完候補一覧 / List of completion candidates
        items: Vec<LspCompletionItem>,
    },
    /// ホバー結果 / Hover result
    #[serde(rename = "hover")]
    Hover {
        /// ホバー情報（存在しない場合はNone） / Hover info (None if unavailable)
        info: Option<LspHoverInfo>,
    },
    /// 診断結果 / Diagnostics result
    #[serde(rename = "diagnostics")]
    Diagnostics {
        /// 診断アイテム一覧 / List of diagnostic items
        items: Vec<LspDiagnosticItem>,
    },
    /// 定義ジャンプ結果 / Go-to-definition result
    #[serde(rename = "goto_definition")]
    GotoDefinition {
        /// 定義の位置（見つからない場合はNone） / Definition location (None if not found)
        location: Option<LspLocationSpan>,
    },
    /// ドキュメントシンボル結果 / Document symbols result
    #[serde(rename = "document_symbols")]
    DocumentSymbols {
        /// シンボル一覧 / List of symbols
        items: Vec<LspSymbolItem>,
    },
}

/// サーバーからのレスポンス
/// Response from the server
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    /// 処理成功フラグ / Success flag
    pub success: bool,
    /// 成功時のメッセージ / Message on success
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// エラー時のメッセージ / Error message on failure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// MIDIポート一覧 / List of MIDI ports
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<PortInfo>>,
    /// LSP結果
    /// LSP result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsp: Option<LspResult>,
}

impl Response {
    /// 成功レスポンス
    /// Creates a success response
    ///
    /// # 引数 / Arguments
    /// * `message` - 成功メッセージ / Success message
    ///
    /// # 戻り値 / Returns
    /// 成功フラグが立ったレスポンス / Response with success flag set
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
    /// Creates an error response
    ///
    /// # 引数 / Arguments
    /// * `error` - エラーメッセージ / Error message
    ///
    /// # 戻り値 / Returns
    /// 失敗フラグが立ったレスポンス / Response with success flag unset
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
    /// Creates a port list response
    ///
    /// # 引数 / Arguments
    /// * `ports` - MIDIポート情報の一覧 / List of MIDI port information
    ///
    /// # 戻り値 / Returns
    /// ポート一覧を含むレスポンス / Response containing the port list
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
    /// Creates an LSP result response
    ///
    /// # 引数 / Arguments
    /// * `result` - LSP処理結果 / LSP processing result
    ///
    /// # 戻り値 / Returns
    /// LSP結果を含むレスポンス / Response containing the LSP result
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
