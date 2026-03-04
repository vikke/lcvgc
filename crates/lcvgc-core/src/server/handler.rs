use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::evaluator::Evaluator;
use crate::lsp::analyzer::LspAnalyzer;
use crate::lsp::context::{
    build_completion_items, determine_completion_context, offset_to_line_col, word_at_offset,
};
use crate::lsp::diagnostic::DiagnosticProvider;
use crate::lsp::goto_def::GotoDefinitionProvider;
use crate::lsp::hover::HoverProvider;
use crate::lsp::symbols::DocumentSymbolProvider;

use crate::midi;

use super::protocol::{
    LspCompletionItem, LspDiagnosticItem, LspHoverInfo, LspLocationSpan, LspResult, LspSymbolItem,
    PortInfo, Request, Response,
};

/// リクエストを処理してレスポンスを返す
pub async fn handle_request(evaluator: &Arc<Mutex<Evaluator>>, request: Request) -> Response {
    match request {
        Request::Eval { source } => {
            let mut ev = evaluator.lock().await;
            match ev.eval_source(&source) {
                Ok(results) => {
                    let msg = results
                        .iter()
                        .map(|r| format!("{:?}", r))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Response::ok(msg)
                }
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::Load { path } => {
            let mut ev = evaluator.lock().await;
            match ev.load_file(&path) {
                Ok(results) => Response::ok(format!("{} blocks evaluated", results.len())),
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::Status => {
            let ev = evaluator.lock().await;
            Response::ok(format!(
                "BPM: {:.1}, State: {:?}",
                ev.bpm(),
                ev.state().state()
            ))
        }
        Request::ListPorts => {
            match (midi::port::list_ports(), midi::port::list_input_ports()) {
                (Ok(outputs), Ok(inputs)) => {
                    let mut ports: Vec<PortInfo> = outputs
                        .into_iter()
                        .map(|name| PortInfo {
                            name,
                            direction: "out".to_string(),
                        })
                        .collect();
                    ports.extend(inputs.into_iter().map(|name| PortInfo {
                        name,
                        direction: "in".to_string(),
                    }));
                    Response::ports(ports)
                }
                (Err(e), _) | (_, Err(e)) => Response::err(e.to_string()),
            }
        }
        Request::LspCompletion { source, offset } => {
            let mut analyzer = LspAnalyzer::new();
            analyzer.update(source);
            let ctx = determine_completion_context(analyzer.source(), offset);
            let items = build_completion_items(&ctx, analyzer.registry());
            let lsp_items: Vec<LspCompletionItem> = items
                .into_iter()
                .map(|item| LspCompletionItem {
                    label: item.label,
                    detail: item.detail,
                    kind: format!("{:?}", item.kind),
                })
                .collect();
            Response::lsp(LspResult::Completion { items: lsp_items })
        }
        Request::LspHover { source, offset } => {
            let mut analyzer = LspAnalyzer::new();
            analyzer.update(source);
            let info = analyzer
                .block_at_offset(offset)
                .and_then(|sb| HoverProvider::hover_content(sb))
                .map(|content| LspHoverInfo { content });
            Response::lsp(LspResult::Hover { info })
        }
        Request::LspDiagnostics { source } => {
            let mut analyzer = LspAnalyzer::new();
            analyzer.update(source.clone());
            let mut diags = DiagnosticProvider::from_parse_errors(analyzer.errors());
            diags.extend(DiagnosticProvider::undefined_references(
                analyzer.blocks(),
                analyzer.registry(),
            ));
            let items: Vec<LspDiagnosticItem> = diags
                .into_iter()
                .map(|d| {
                    let (start_line, start_col) = offset_to_line_col(&source, d.span.start);
                    let (end_line, end_col) = offset_to_line_col(&source, d.span.end);
                    LspDiagnosticItem {
                        start_line,
                        start_col,
                        end_line,
                        end_col,
                        message: d.message,
                        severity: format!("{:?}", d.severity),
                    }
                })
                .collect();
            Response::lsp(LspResult::Diagnostics { items })
        }
        Request::LspGotoDefinition { source, offset } => {
            let mut analyzer = LspAnalyzer::new();
            analyzer.update(source.clone());
            let location = word_at_offset(&source, offset)
                .and_then(|word| {
                    GotoDefinitionProvider::find_definition(&word, analyzer.blocks())
                })
                .map(|span| {
                    let (start_line, start_col) = offset_to_line_col(&source, span.start);
                    let (end_line, end_col) = offset_to_line_col(&source, span.end);
                    LspLocationSpan {
                        start_line,
                        start_col,
                        end_line,
                        end_col,
                    }
                });
            Response::lsp(LspResult::GotoDefinition { location })
        }
        Request::LspDocumentSymbols { source } => {
            let mut analyzer = LspAnalyzer::new();
            analyzer.update(source.clone());
            let items: Vec<LspSymbolItem> = DocumentSymbolProvider::symbols(analyzer.blocks())
                .into_iter()
                .map(|sym| {
                    let (start_line, start_col) = offset_to_line_col(&source, sym.span.start);
                    let (end_line, end_col) = offset_to_line_col(&source, sym.span.end);
                    LspSymbolItem {
                        name: sym.name,
                        kind: format!("{:?}", sym.kind),
                        start_line,
                        start_col,
                        end_line,
                        end_col,
                    }
                })
                .collect();
            Response::lsp(LspResult::DocumentSymbols { items })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_eval_success() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Eval {
            source: "tempo 140".into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        assert!(resp.message.unwrap().contains("TempoChanged"));
    }

    #[tokio::test]
    async fn handle_eval_parse_error() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Eval {
            source: "invalid !@# syntax".into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn handle_load_not_found() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Load {
            path: "/nonexistent/file.cvg".into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn handle_status() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Status;
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let msg = resp.message.unwrap();
        assert!(msg.contains("BPM: 120.0"));
        assert!(msg.contains("Stopped"));
    }

    #[tokio::test]
    #[ignore] // 実MIDIハードウェアが必要
    async fn handle_list_ports() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::ListPorts;
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        assert!(resp.ports.is_some());
    }
}
