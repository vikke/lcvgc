use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use lcvgc_core::ast::Block;
use crate::analyzer::LspAnalyzer;
use crate::completion::{CompletionKind, CompletionProvider};
use crate::diagnostic::DiagnosticProvider;

pub struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<String, LspAnalyzer>>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn publish_diagnostics(&self, uri: Url, analyzer: &LspAnalyzer) {
        let mut diags = DiagnosticProvider::from_parse_errors(analyzer.errors());
        diags.extend(DiagnosticProvider::undefined_references(
            analyzer.blocks(),
            analyzer.registry(),
        ));

        let lsp_diags: Vec<Diagnostic> = diags
            .iter()
            .map(|d| {
                let range = offset_to_range(analyzer.source(), d.span.start, d.span.end);
                Diagnostic {
                    range,
                    severity: Some(match d.severity {
                        crate::diagnostic::DiagnosticSeverity::Error => {
                            DiagnosticSeverity::ERROR
                        }
                        crate::diagnostic::DiagnosticSeverity::Warning => {
                            DiagnosticSeverity::WARNING
                        }
                    }),
                    message: d.message.clone(),
                    ..Default::default()
                }
            })
            .collect();

        self.client
            .publish_diagnostics(uri, lsp_diags, None)
            .await;
    }
}

/// バイトオフセットからLSP Positionへ変換
fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position {
        line,
        character: col,
    }
}

/// バイトオフセット範囲からLSP Rangeへ変換
fn offset_to_range(source: &str, start: usize, end: usize) -> Range {
    let start_pos = offset_to_position(source, start);
    let end_pos = offset_to_position(source, end);
    Range {
        start: start_pos,
        end: end_pos,
    }
}

/// CompletionKindからlsp_types::CompletionItemKindへ変換
fn map_completion_kind(kind: CompletionKind) -> CompletionItemKind {
    match kind {
        CompletionKind::Keyword => CompletionItemKind::KEYWORD,
        CompletionKind::NoteName => CompletionItemKind::VALUE,
        CompletionKind::ChordName => CompletionItemKind::VALUE,
        CompletionKind::CcAlias => CompletionItemKind::PROPERTY,
        CompletionKind::Identifier => CompletionItemKind::VARIABLE,
    }
}

/// Block種別からlsp_types::SymbolKindへ変換
fn map_block_to_symbol_kind(block: &Block) -> SymbolKind {
    match block {
        Block::Device(_) => SymbolKind::MODULE,
        Block::Instrument(_) => SymbolKind::CLASS,
        Block::Kit(_) => SymbolKind::CLASS,
        Block::Clip(_) => SymbolKind::FUNCTION,
        Block::Scene(_) => SymbolKind::NAMESPACE,
        Block::Session(_) => SymbolKind::NAMESPACE,
        Block::Tempo(_) => SymbolKind::CONSTANT,
        Block::Scale(_) => SymbolKind::CONSTANT,
        Block::Var(_) => SymbolKind::VARIABLE,
        Block::Include(_) => SymbolKind::FILE,
        Block::Play(_) | Block::Stop(_) => SymbolKind::EVENT,
    }
}

/// Blockから名前を取得
fn block_name(block: &Block) -> String {
    match block {
        Block::Device(d) => d.name.clone(),
        Block::Instrument(i) => i.name.clone(),
        Block::Kit(k) => k.name.clone(),
        Block::Clip(c) => c.name.clone(),
        Block::Scene(s) => s.name.clone(),
        Block::Session(s) => s.name.clone(),
        Block::Tempo(t) => format!("tempo {:?}", t),
        Block::Scale(s) => format!("scale {:?} {:?}", s.root, s.scale_type),
        Block::Var(v) => v.name.clone(),
        Block::Include(i) => i.path.clone(),
        Block::Play(p) => format!("play {:?}", p.target),
        Block::Stop(s) => format!("stop {:?}", s.target),
    }
}

/// カーソル位置周辺の識別子を抽出
fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    if offset > source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';

    let mut start = offset;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }
    if start == end {
        None
    } else {
        Some(source[start..end].to_string())
    }
}

/// LSP Position からバイトオフセットへ変換
fn position_to_offset(source: &str, pos: Position) -> usize {
    let mut line = 0u32;
    for (i, ch) in source.char_indices() {
        if line == pos.line {
            let line_start = i;
            return line_start + pos.character as usize;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    source.len()
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![" ".into(), "{".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "lcvgc LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        let mut analyzer = LspAnalyzer::new();
        analyzer.update(text);

        let mut docs = self.documents.write().await;
        docs.insert(uri.to_string(), analyzer);
        let analyzer = docs.get(uri.as_str()).unwrap();
        self.publish_diagnostics(uri, analyzer).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().next() {
            let mut docs = self.documents.write().await;
            if let Some(analyzer) = docs.get_mut(uri.as_str()) {
                analyzer.update(change.text);
                self.publish_diagnostics(uri, analyzer).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(uri.as_str());
        self.client
            .publish_diagnostics(uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let docs = self.documents.read().await;
        let _analyzer = match docs.get(uri.as_str()) {
            Some(a) => a,
            None => return Ok(None),
        };

        let mut items: Vec<CompletionItem> = Vec::new();

        // キーワード補完
        for ci in CompletionProvider::keyword_completions() {
            items.push(CompletionItem {
                label: ci.label,
                detail: ci.detail,
                kind: Some(map_completion_kind(ci.kind)),
                ..Default::default()
            });
        }

        // ノート名補完
        for ci in CompletionProvider::note_completions() {
            items.push(CompletionItem {
                label: ci.label,
                detail: ci.detail,
                kind: Some(map_completion_kind(ci.kind)),
                ..Default::default()
            });
        }

        // 標準CC補完
        for ci in CompletionProvider::standard_cc_completions() {
            items.push(CompletionItem {
                label: ci.label,
                detail: ci.detail,
                kind: Some(map_completion_kind(ci.kind)),
                ..Default::default()
            });
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let analyzer = match docs.get(uri.as_str()) {
            Some(a) => a,
            None => return Ok(None),
        };

        let offset = position_to_offset(analyzer.source(), pos);
        let block = match analyzer.block_at_offset(offset) {
            Some(b) => b,
            None => return Ok(None),
        };

        let name = block_name(&block.block);
        let kind = format!("{:?}", block.block).split('(').next().unwrap_or("").to_string();
        let value = format!("**{}** `{}`", kind, name);

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let analyzer = match docs.get(uri.as_str()) {
            Some(a) => a,
            None => return Ok(None),
        };

        let offset = position_to_offset(analyzer.source(), pos);
        let word = match word_at_offset(analyzer.source(), offset) {
            Some(w) => w,
            None => return Ok(None),
        };

        // ブロック名でマッチするものを探す
        for sb in analyzer.blocks() {
            let name = block_name(&sb.block);
            if name == word {
                let range = offset_to_range(analyzer.source(), sb.span.start, sb.span.end);
                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range,
                })));
            }
        }

        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let docs = self.documents.read().await;
        let analyzer = match docs.get(uri.as_str()) {
            Some(a) => a,
            None => return Ok(None),
        };

        #[allow(deprecated)]
        let symbols: Vec<SymbolInformation> = analyzer
            .blocks()
            .iter()
            .map(|sb| {
                let range = offset_to_range(analyzer.source(), sb.span.start, sb.span.end);
                SymbolInformation {
                    name: block_name(&sb.block),
                    kind: map_block_to_symbol_kind(&sb.block),
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range,
                    },
                    container_name: None,
                }
            })
            .collect();

        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_to_position_start() {
        let pos = offset_to_position("hello\nworld", 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn offset_to_position_second_line() {
        let pos = offset_to_position("hello\nworld", 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn offset_to_position_middle_second_line() {
        let pos = offset_to_position("hello\nworld", 9);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 3);
    }

    #[test]
    fn offset_to_position_end() {
        let pos = offset_to_position("hello\nworld", 11);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 5);
    }

    #[test]
    fn offset_to_position_empty() {
        let pos = offset_to_position("", 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn offset_to_range_single_line() {
        let range = offset_to_range("tempo 120", 0, 9);
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, 9);
    }

    #[test]
    fn map_completion_kind_keyword() {
        assert_eq!(
            map_completion_kind(CompletionKind::Keyword),
            CompletionItemKind::KEYWORD
        );
    }

    #[test]
    fn map_completion_kind_note() {
        assert_eq!(
            map_completion_kind(CompletionKind::NoteName),
            CompletionItemKind::VALUE
        );
    }

    #[test]
    fn map_completion_kind_chord() {
        assert_eq!(
            map_completion_kind(CompletionKind::ChordName),
            CompletionItemKind::VALUE
        );
    }

    #[test]
    fn map_completion_kind_cc() {
        assert_eq!(
            map_completion_kind(CompletionKind::CcAlias),
            CompletionItemKind::PROPERTY
        );
    }

    #[test]
    fn map_completion_kind_identifier() {
        assert_eq!(
            map_completion_kind(CompletionKind::Identifier),
            CompletionItemKind::VARIABLE
        );
    }

    #[test]
    fn map_block_symbol_kind_device() {
        use lcvgc_core::ast::device::DeviceDef;
        let block = Block::Device(DeviceDef {
            name: "d".into(),
            port: "p".into(),
        });
        assert_eq!(map_block_to_symbol_kind(&block), SymbolKind::MODULE);
    }

    #[test]
    fn map_block_symbol_kind_clip() {
        use lcvgc_core::ast::clip::{ClipBody, ClipDef, PitchedClipBody};
        use lcvgc_core::parser::clip_options::ClipOptions;
        let block = Block::Clip(ClipDef {
            name: "c".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        });
        assert_eq!(map_block_to_symbol_kind(&block), SymbolKind::FUNCTION);
    }

    #[test]
    fn map_block_symbol_kind_tempo() {
        use lcvgc_core::ast::tempo::Tempo;
        let block = Block::Tempo(Tempo::Absolute(120));
        assert_eq!(map_block_to_symbol_kind(&block), SymbolKind::CONSTANT);
    }

    #[test]
    fn word_at_offset_middle() {
        assert_eq!(word_at_offset("hello world", 2), Some("hello".into()));
    }

    #[test]
    fn word_at_offset_none_on_space() {
        // offset 1 in "a b" is space, but backward search finds 'a'
        // Use a string where space is surrounded by spaces
        assert_eq!(word_at_offset(" a ", 0), None);
    }

    #[test]
    fn position_to_offset_first_line() {
        let off = position_to_offset("hello\nworld", Position { line: 0, character: 3 });
        assert_eq!(off, 3);
    }

    #[test]
    fn position_to_offset_second_line() {
        let off = position_to_offset("hello\nworld", Position { line: 1, character: 2 });
        assert_eq!(off, 8);
    }
}
