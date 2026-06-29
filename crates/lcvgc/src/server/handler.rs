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

/// リクエストを処理する（panic 防壁付き）。
///
/// 実処理は `handle_request_inner` に委譲し、その実行を別タスクへ隔離する。
/// パース器など下流コードが万一 panic しても、その panic を `JoinError` 経由で
/// 捕捉して `Response::err` に変換することで、呼び出し元の接続処理タスク
/// (`server::run_server` の受信ループ) が巻き込まれて切断されるのを防ぐ。
///
/// これがないと、診断リクエスト処理中の panic で接続が切れ、エディタ側に
/// LSP 応答が返らなくなり「一度出たエラー表示が消えない」不具合に繋がる。
///
/// Wraps `handle_request_inner` with a panic guard: the inner work runs in a
/// spawned task so that a panic in downstream code (e.g. the parser) is caught
/// via `JoinError` and turned into `Response::err`, instead of tearing down the
/// caller's connection loop and dropping the client.
///
/// # 引数 / Arguments
/// * `evaluator` - 共有された DSL 評価エンジン / Shared DSL evaluator
/// * `request` - 処理対象のリクエスト / The request to handle
///
/// # 戻り値 / Returns
/// 処理結果のレスポンス（panic 時はエラーレスポンス）
/// The response (an error response if the inner handler panicked)
pub async fn handle_request(evaluator: &Arc<Mutex<Evaluator>>, request: Request) -> Response {
    let ev = evaluator.clone();
    // 別タスクへ隔離して panic を JoinError として捕捉する。
    // Isolate into a spawned task so panics surface as a JoinError.
    match tokio::spawn(async move { handle_request_inner(&ev, request).await }).await {
        Ok(resp) => resp,
        Err(join_err) => {
            // panic 由来のみ握りつぶす。キャンセル等は想定しないが安全側で扱う。
            // Only swallow panics; other join errors are not expected here.
            tracing::error!("handle_request inner task panicked: {join_err}");
            Response::err(format!(
                "internal error: request handler panicked ({join_err})"
            ))
        }
    }
}

/// リクエストの実処理本体。
///
/// 旧 `handle_request` の中身。panic 防壁は呼び出し元の `handle_request` が担う。
/// The actual request-handling body (formerly `handle_request`); the panic
/// guard lives in the wrapping `handle_request`.
async fn handle_request_inner(evaluator: &Arc<Mutex<Evaluator>>, request: Request) -> Response {
    match request {
        Request::Eval { source } => {
            // ロック外 eval (prepare/apply 分離): 重い parse+compile を再生ドライバと
            // 同じ Evaluator ロックの外で行い、apply だけ短時間ロックする。
            // Off-lock eval (prepare/apply split): heavy parse+compile runs outside
            // the shared Evaluator lock; only apply takes a short lock.
            let snapshot = { evaluator.lock().await.snapshot_for_prepare() };
            let prepared = match snapshot.prepare_source(&source) {
                Ok(prepared) => prepared,
                Err(e) => return Response::err(e.to_string()),
            };
            let apply_result = { evaluator.lock().await.apply_prepared(prepared) };
            match apply_result {
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
        Request::Preload { source } => {
            // ロック外 eval (prepare/apply 分離)。preload は play/stop を除外する。
            // Off-lock eval (prepare/apply split); preload excludes play/stop.
            let snapshot = { evaluator.lock().await.snapshot_for_prepare() };
            let prepared = match snapshot.prepare_source_preload(&source) {
                Ok(prepared) => prepared,
                Err(e) => return Response::err(e.to_string()),
            };
            let apply_result = { evaluator.lock().await.apply_prepared(prepared) };
            match apply_result {
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
        Request::Status => {
            let ev = evaluator.lock().await;
            Response::ok(format!(
                "BPM: {:.1}, State: {:?}",
                ev.bpm(),
                ev.state().state()
            ))
        }
        Request::ListPorts => match (midi::port::list_ports(), midi::port::list_input_ports()) {
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
        },
        Request::LspCompletion {
            source,
            offset,
            include_sources,
        } => {
            // registryが空ならソースからプリロード
            // Preload from source if registry is empty
            let mut ev = evaluator.lock().await;
            let additional: Vec<&str> = include_sources
                .as_ref()
                .map(|incs| incs.iter().map(|i| i.source.as_str()).collect())
                .unwrap_or_default();
            ev.preload_from_source(&source, &additional);
            let mut analyzer = LspAnalyzer::with_base_registry(ev.registry_snapshot());
            drop(ev);
            if let Some(ref includes) = include_sources {
                analyzer.update_with_include_sources(source, includes);
            } else {
                analyzer.update(source);
            }
            let ctx = determine_completion_context(analyzer.source(), offset);
            let items = build_completion_items(&ctx, analyzer.registry());
            let lsp_items: Vec<LspCompletionItem> = items
                .into_iter()
                .map(|item| LspCompletionItem {
                    label: item.label,
                    detail: item.detail,
                    kind: format!("{:?}", item.kind),
                    sort_text: item.sort_text,
                })
                .collect();
            Response::lsp(LspResult::Completion { items: lsp_items })
        }
        Request::LspHover {
            source,
            offset,
            include_sources,
        } => {
            // registryが空ならソースからプリロード
            // Preload from source if registry is empty
            let mut ev = evaluator.lock().await;
            let additional: Vec<&str> = include_sources
                .as_ref()
                .map(|incs| incs.iter().map(|i| i.source.as_str()).collect())
                .unwrap_or_default();
            ev.preload_from_source(&source, &additional);
            let mut analyzer = LspAnalyzer::with_base_registry(ev.registry_snapshot());
            drop(ev);
            if let Some(ref includes) = include_sources {
                analyzer.update_with_include_sources(source, includes);
            } else {
                analyzer.update(source);
            }
            let info = analyzer
                .block_at_offset(offset)
                .and_then(HoverProvider::hover_content)
                .map(|content| LspHoverInfo { content });
            Response::lsp(LspResult::Hover { info })
        }
        Request::LspDiagnostics {
            source,
            include_sources,
        } => {
            // registryが空ならソースからプリロード
            // Preload from source if registry is empty
            let mut ev = evaluator.lock().await;
            let additional: Vec<&str> = include_sources
                .as_ref()
                .map(|incs| incs.iter().map(|i| i.source.as_str()).collect())
                .unwrap_or_default();
            ev.preload_from_source(&source, &additional);
            let mut analyzer = LspAnalyzer::with_base_registry(ev.registry_snapshot());
            // PR #55: device 接続失敗 diagnostic 用に Evaluator のエラー状態を退避
            // PR #55: snapshot the device connection errors for diagnostic generation
            let device_connection_errors = ev.device_connection_errors().clone();
            drop(ev);
            // include_sourcesがある場合はinclude解決付きで更新、ない場合は従来通り
            // Use include resolution when include_sources is provided, otherwise use standard update
            if let Some(ref includes) = include_sources {
                analyzer.update_with_include_sources(source.clone(), includes);
            } else {
                analyzer.update(source.clone());
            }
            let mut diags = DiagnosticProvider::from_parse_errors(analyzer.errors());
            diags.extend(DiagnosticProvider::undefined_references(
                analyzer.blocks(),
                analyzer.registry(),
            ));
            // includeの位置チェック（先頭以外はエラー）
            // Check include position (non-top includes are errors)
            diags.extend(DiagnosticProvider::include_position_diagnostics(
                analyzer.blocks(),
            ));
            // §10.4: pause / resume の target 名が未定義なら Warning
            // §10.4: Warn on pause / resume with unknown target names
            diags.extend(DiagnosticProvider::pause_resume_target_diagnostics(
                analyzer.blocks(),
                analyzer.registry(),
            ));
            // §10.4: mute / unmute の target 名が未定義の clip なら Warning
            // §10.4: Warn on mute / unmute with unknown clip target names
            diags.extend(DiagnosticProvider::mute_unmute_target_diagnostics(
                analyzer.blocks(),
                analyzer.registry(),
            ));
            // PR #55: device の MIDI ポート接続失敗を Error 診断として加える
            // PR #55: surface device MIDI port connection failures as Error diagnostics
            diags.extend(DiagnosticProvider::device_connection_diagnostics(
                analyzer.blocks(),
                &device_connection_errors,
            ));
            // arp の音価未指定 (`[..]:D` も `arp(_, N)` も無し) を Error 診断として加える
            // Surface arpeggio specifications missing both per-step duration sources.
            diags.extend(DiagnosticProvider::arpeggio_missing_duration_diagnostics(
                analyzer.blocks(),
            ));
            // include_diagnostics()は呼ばない（Lua側で実施）
            // Do not call include_diagnostics() (handled on Lua side)
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
        Request::LspGotoDefinition {
            source,
            offset,
            include_sources,
        } => {
            // registryが空ならソースからプリロード
            // Preload from source if registry is empty
            let mut ev = evaluator.lock().await;
            let additional: Vec<&str> = include_sources
                .as_ref()
                .map(|incs| incs.iter().map(|i| i.source.as_str()).collect())
                .unwrap_or_default();
            ev.preload_from_source(&source, &additional);
            let mut analyzer = LspAnalyzer::with_base_registry(ev.registry_snapshot());
            drop(ev);
            if let Some(ref includes) = include_sources {
                analyzer.update_with_include_sources(source.clone(), includes);
            } else {
                analyzer.update(source.clone());
            }
            let location = word_at_offset(&source, offset)
                .and_then(|word| GotoDefinitionProvider::find_definition(&word, analyzer.blocks()))
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
        Request::LspDocumentSymbols {
            source,
            include_sources,
        } => {
            // registryが空ならソースからプリロード
            // Preload from source if registry is empty
            let mut ev = evaluator.lock().await;
            let additional: Vec<&str> = include_sources
                .as_ref()
                .map(|incs| incs.iter().map(|i| i.source.as_str()).collect())
                .unwrap_or_default();
            ev.preload_from_source(&source, &additional);
            let mut analyzer = LspAnalyzer::with_base_registry(ev.registry_snapshot());
            drop(ev);
            if let Some(ref includes) = include_sources {
                analyzer.update_with_include_sources(source.clone(), includes);
            } else {
                analyzer.update(source.clone());
            }
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
        // MIDI 入力購読系は接続レイヤ (`server::handle_connection`) で処理する。
        // 出力集約チャネルと購読ハンドルのライフタイムが接続スコープに紐づくため、
        // ステートレスな本ハンドラでは扱えない。万一ここへ到達したら設計不整合
        // なので、接続を切らずに明示的なエラーレスポンスを返す。
        //
        // MIDI input subscription requests are handled in the connection layer
        // (`server::handle_connection`), since the outbound channel and the
        // subscription handle are scoped to the connection. Reaching this
        // stateless handler indicates a routing bug; return an explicit error.
        Request::SubscribeMidiIn { .. } | Request::UnsubscribeMidiIn => Response::err(
            "MIDI 入力購読リクエストは接続レイヤで処理されます / \
             MIDI input subscription requests are handled at the connection layer",
        ),
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

    /// 防壁回帰テスト: panic 防壁 (spawn 隔離) を通しても、正常リクエストの
    /// 応答内容が従来と変わらないことを検証する。
    /// Regression: routing through the panic guard does not change normal responses.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_request_guard_preserves_normal_response() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let resp = handle_request(
            &ev,
            Request::Eval {
                source: "tempo 140".into(),
            },
        )
        .await;
        assert!(resp.success);
        assert!(resp.message.unwrap().contains("TempoChanged"));
    }

    /// 防壁コアの検証: inner 相当の処理が panic しても、`tokio::spawn` で隔離して
    /// いるため `JoinError(is_panic)` として捕捉でき、`handle_request` と同じ要領で
    /// `Response::err` に変換できる（=呼び出し元タスクは巻き込まれない）。
    ///
    /// `handle_request` の panic 経路を入力で再現するのは困難なため、防壁の構造
    /// （spawn → JoinError → err 変換）を直接検証する。
    ///
    /// Verifies the guard core: a panic in the spawned task surfaces as a
    /// `JoinError(is_panic)` and can be converted into an error response,
    /// mirroring `handle_request`'s recovery path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawned_panic_is_caught_as_join_error() {
        let join = tokio::spawn(async move {
            panic!("simulated downstream panic");
        });
        let result: Result<(), _> = join.await;
        let err = result.expect_err("panic は JoinError になるはず");
        assert!(err.is_panic(), "JoinError は panic 由来であるべき");

        // handle_request と同じ変換でエラーレスポンスを作れること
        // The same conversion handle_request uses yields an error response.
        let resp = Response::err(format!("internal error: request handler panicked ({err})"));
        assert!(!resp.success);
        assert!(resp.error.unwrap().contains("panicked"));
    }

    /// preloadリクエストでplay/stopがスキップされることを検証する
    /// Verifies that preload request skips play/stop blocks
    #[tokio::test]
    async fn handle_preload_skips_play_stop() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let source = "tempo 140\n\nscene test_scene {}\n\nplay test_scene\nstop\n";
        let req = Request::Preload {
            source: source.into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let msg = resp.message.unwrap();
        assert!(msg.contains("TempoChanged"));
        assert!(msg.contains("Scene"));
        assert!(!msg.contains("PlayStarted"));
        assert!(!msg.contains("Stopped"));
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

    /// トップレベルでのLSP補完リクエストでキーワード補完が返ることを検証する
    #[tokio::test]
    async fn handle_lsp_completion_toplevel() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::LspCompletion {
            source: "".into(),
            offset: 0,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::Completion { items } => {
                assert!(!items.is_empty());
                // トップレベルではキーワード補完が返る
                assert!(items.iter().any(|i| i.label == "tempo"));
                assert!(items.iter().any(|i| i.label == "device"));
            }
            _ => panic!("Expected Completion"),
        }
    }

    /// tempoキーワードのLSPホバーで値を含む情報が返ることを検証する
    #[tokio::test]
    async fn handle_lsp_hover_tempo() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::LspHover {
            source: "tempo 120".into(),
            offset: 3,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::Hover { info } => {
                assert!(info.is_some());
                assert!(info.unwrap().content.contains("120"));
            }
            _ => panic!("Expected Hover"),
        }
    }

    /// 有効なDSLソースのLSP診断リクエストで診断アイテムが空であることを検証する
    #[tokio::test]
    async fn handle_lsp_diagnostics_valid() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::LspDiagnostics {
            source: "tempo 120".into(),
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::Diagnostics { items } => {
                assert!(items.is_empty());
            }
            _ => panic!("Expected Diagnostics"),
        }
    }

    /// 統合: resolution < 4 の壊れたドラム clip でも LSP 診断リクエストが
    /// panic で落ちず、診断付きの成功レスポンスを返すことを検証する。
    /// (A) パース段階で構文エラー化, (B) 防壁の二段構えが効くことの end-to-end 確認。
    /// これが効かないと接続が切れ「エラー表示が消えない」症状になる。
    ///
    /// Integration: a broken drum clip with resolution < 4 no longer panics the
    /// diagnostics request; it returns a successful response carrying diagnostics.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_lsp_diagnostics_broken_drum_resolution_does_not_panic() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let source =
            "clip d [bars 1] {\n  use tr808\n  resolution 1\n\n  cp oooo | oooo\n}\n".to_string();
        let req = Request::LspDiagnostics {
            source,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        // 接続が切れず、成功レスポンスが返ること
        // A successful response is returned (connection stays alive).
        assert!(resp.success, "壊れた入力でも success レスポンスが返るべき");
        match resp.lsp.unwrap() {
            super::super::protocol::LspResult::Diagnostics { items } => {
                // 構文エラーが診断として 1 件以上含まれること
                // At least one syntax-error diagnostic is reported.
                assert!(!items.is_empty(), "構文エラー診断が含まれるべき");
            }
            _ => panic!("Expected Diagnostics"),
        }
    }

    /// 統合: 壊れたドラム clip の診断後、修正済みソースを再診断すると診断が
    /// 空になり「エラー表示が消える」ことを検証する。
    /// Integration: after a broken request, re-diagnosing fixed source clears it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_lsp_diagnostics_recovers_after_fix() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));

        // 1) 壊れた状態
        let broken = "clip d [bars 1] {\n  use tr808\n  resolution 1\n  cp oooo\n}\n".to_string();
        let r1 = handle_request(
            &ev,
            Request::LspDiagnostics {
                source: broken,
                include_sources: None,
            },
        )
        .await;
        assert!(r1.success);

        // 2) 修正後 (resolution を 16 に直す)
        let fixed =
            "clip d [bars 1] {\n  use tr808\n  resolution 16\n  cp oooo | oooo | oooo | oooo\n}\n"
                .to_string();
        let r2 = handle_request(
            &ev,
            Request::LspDiagnostics {
                source: fixed,
                include_sources: None,
            },
        )
        .await;
        assert!(r2.success);
        match r2.lsp.unwrap() {
            super::super::protocol::LspResult::Diagnostics { items } => {
                assert!(
                    items.is_empty(),
                    "修正後は診断が空になるべき (エラーが消える)"
                );
            }
            _ => panic!("Expected Diagnostics"),
        }
    }

    /// instrument内でdevice参照のLSP定義ジャンプが定義箇所を返すことを検証する
    #[tokio::test]
    async fn handle_lsp_goto_definition_device() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let source =
            "device synth {\n  port \"IAC\"\n}\ninstrument bass {\n  device synth\n  channel 1\n}";
        let req = Request::LspGotoDefinition {
            source: source.into(),
            // "synth" in instrument block at offset ~55
            offset: source.find("device synth\n  channel").unwrap() + 7,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::GotoDefinition { location } => {
                assert!(location.is_some());
                let loc = location.unwrap();
                // device synth is at line 0
                assert_eq!(loc.start_line, 0);
            }
            _ => panic!("Expected GotoDefinition"),
        }
    }

    /// tempoを含むDSLのLSPドキュメントシンボルでTempoシンボルが返ることを検証する
    #[tokio::test]
    async fn handle_lsp_document_symbols_tempo() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::LspDocumentSymbols {
            source: "tempo 120".into(),
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::DocumentSymbols { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].kind, "Tempo");
            }
            _ => panic!("Expected DocumentSymbols"),
        }
    }

    /// include_sourcesでクリップがsceneから参照されても偽エラーが出ないことを検証する
    /// Verifies that clips from include_sources don't cause false "undefined clip" errors in scenes
    #[tokio::test]
    async fn handle_lsp_diagnostics_with_include_sources_resolves_clips() {
        use crate::server::protocol::IncludeSource;

        let source = "include bass.cvg\ndevice synth {\n  port \"IAC\"\n}\ninstrument inst {\n  device synth\n  channel 1\n}\nscene main {\n  inst: bass\n}";
        let include_sources = vec![IncludeSource {
            path: "bass.cvg".into(),
            source: "clip bass {\n  c4\n}".into(),
        }];

        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::LspDiagnostics {
            source: source.into(),
            include_sources: Some(include_sources),
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::Diagnostics { items } => {
                // include_sourcesのclipが解決されるため、未定義エラーは出ない
                // No undefined errors because clips from include_sources are resolved
                let undef_errors: Vec<_> = items
                    .iter()
                    .filter(|i| i.message.contains("未定義"))
                    .collect();
                assert!(
                    undef_errors.is_empty(),
                    "Expected no undefined clip errors, but got: {:?}",
                    undef_errors
                );
            }
            _ => panic!("Expected Diagnostics"),
        }
    }

    /// include_sourcesなしの場合、既存の動作と同じであることを検証する
    /// Verifies that behavior without include_sources remains the same
    #[tokio::test]
    async fn handle_lsp_diagnostics_without_include_sources_unchanged() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::LspDiagnostics {
            source: "tempo 120".into(),
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let lsp = resp.lsp.unwrap();
        match lsp {
            super::super::protocol::LspResult::Diagnostics { items } => {
                assert!(items.is_empty());
            }
            _ => panic!("Expected Diagnostics"),
        }
    }

    // -----------------------------------------------------------------
    // PR #83: LSP ハンドラ経由の preload は device 接続副作用を起こさない
    //
    // 各 LSP ハンドラは `preload_from_source` を経由して device ブロック
    // を含むソースを解析するが、これは静的解析であり物理的な MIDI 接続
    // を試みるべきでない。`Evaluator::set_device_event_tx` で仕掛けた
    // tx に何も届かないこと（= `DeviceEvent::Upsert` が発火しないこと）
    // を全 LSP ハンドラで確認する。
    //
    // Each LSP handler calls `preload_from_source` to analyse sources
    // containing `device` blocks. Since the analysis is purely static,
    // it must not attempt physical MIDI connections. We assert that the
    // `DeviceEvent` channel set via `set_device_event_tx` stays empty.
    // -----------------------------------------------------------------

    /// LSP Diagnostics ハンドラ呼び出しで DeviceEvent が emit されない
    /// LSP `Diagnostics` handler must not emit `DeviceEvent`.
    #[tokio::test]
    async fn handle_lsp_diagnostics_does_not_emit_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev_inner = Evaluator::new(120.0);
        ev_inner.set_device_event_tx(tx);
        let ev = Arc::new(Mutex::new(ev_inner));

        let source = "device my_synth {\n  port IAC Driver\n}\n";
        let req = Request::LspDiagnostics {
            source: source.into(),
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);

        assert!(
            rx.try_recv().is_err(),
            "LspDiagnostics handler must not emit DeviceEvent"
        );
    }

    /// LSP Completion ハンドラ呼び出しで DeviceEvent が emit されない
    /// LSP `Completion` handler must not emit `DeviceEvent`.
    #[tokio::test]
    async fn handle_lsp_completion_does_not_emit_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev_inner = Evaluator::new(120.0);
        ev_inner.set_device_event_tx(tx);
        let ev = Arc::new(Mutex::new(ev_inner));

        let source = "device my_synth {\n  port IAC Driver\n}\n";
        let offset = source.len();
        let req = Request::LspCompletion {
            source: source.into(),
            offset,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);

        assert!(
            rx.try_recv().is_err(),
            "LspCompletion handler must not emit DeviceEvent"
        );
    }

    /// LSP Hover ハンドラ呼び出しで DeviceEvent が emit されない
    /// LSP `Hover` handler must not emit `DeviceEvent`.
    #[tokio::test]
    async fn handle_lsp_hover_does_not_emit_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev_inner = Evaluator::new(120.0);
        ev_inner.set_device_event_tx(tx);
        let ev = Arc::new(Mutex::new(ev_inner));

        let source = "device my_synth {\n  port IAC Driver\n}\n";
        // "device" の中の "v" 付近をホバー位置に
        let offset = source.find("device").unwrap() + 3;
        let req = Request::LspHover {
            source: source.into(),
            offset,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);

        assert!(
            rx.try_recv().is_err(),
            "LspHover handler must not emit DeviceEvent"
        );
    }

    /// LSP GotoDefinition ハンドラ呼び出しで DeviceEvent が emit されない
    /// LSP `GotoDefinition` handler must not emit `DeviceEvent`.
    #[tokio::test]
    async fn handle_lsp_goto_definition_does_not_emit_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev_inner = Evaluator::new(120.0);
        ev_inner.set_device_event_tx(tx);
        let ev = Arc::new(Mutex::new(ev_inner));

        let source = "device my_synth {\n  port IAC Driver\n}\ninstrument bass {\n  device my_synth\n  channel 1\n}\n";
        // instrument 内の "my_synth" 参照位置
        let offset = source.rfind("my_synth").unwrap();
        let req = Request::LspGotoDefinition {
            source: source.into(),
            offset,
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);

        assert!(
            rx.try_recv().is_err(),
            "LspGotoDefinition handler must not emit DeviceEvent"
        );
    }

    /// LSP DocumentSymbols ハンドラ呼び出しで DeviceEvent が emit されない
    /// LSP `DocumentSymbols` handler must not emit `DeviceEvent`.
    #[tokio::test]
    async fn handle_lsp_document_symbols_does_not_emit_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev_inner = Evaluator::new(120.0);
        ev_inner.set_device_event_tx(tx);
        let ev = Arc::new(Mutex::new(ev_inner));

        let source = "device my_synth {\n  port IAC Driver\n}\n";
        let req = Request::LspDocumentSymbols {
            source: source.into(),
            include_sources: None,
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);

        assert!(
            rx.try_recv().is_err(),
            "LspDocumentSymbols handler must not emit DeviceEvent"
        );
    }
}
