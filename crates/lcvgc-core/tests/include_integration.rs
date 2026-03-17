//! include機能の統合テスト
//! Integration tests for include functionality

use lcvgc_core::engine::error::EngineError;
use lcvgc_core::engine::evaluator::{EvalResult, Evaluator};

/// 単一ファイルのincludeが正しく展開されることを検証
/// Verifies that a single file include is correctly expanded
#[test]
fn include_single_file() {
    let dir = tempfile::tempdir().unwrap();

    let setup = dir.path().join("setup.cvg");
    std::fs::write(&setup, "tempo 140\n").unwrap();

    let main = dir.path().join("main.cvg");
    std::fs::write(
        &main,
        format!(
            "include {}\n\ndevice synth {{\n  port \"IAC\"\n}}\n",
            setup.display()
        ),
    )
    .unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_file(&main).unwrap();

    // tempoが評価されている
    assert!(results
        .iter()
        .any(|r| matches!(r, EvalResult::TempoChanged(140.0))));
    // deviceが登録されている
    assert!(results
        .iter()
        .any(|r| matches!(r, EvalResult::Registered { kind, .. } if kind == "Device")));
    // IncludeProcessedが返る
    assert!(results
        .iter()
        .any(|r| matches!(r, EvalResult::IncludeProcessed { .. })));
    // BPMが更新されている
    assert!((ev.bpm() - 140.0).abs() < f64::EPSILON);
}

/// 2段階ネストのincludeが正しく展開されることを検証
/// Verifies that two-level nested includes are correctly expanded
#[test]
fn include_nested_two_levels() {
    let dir = tempfile::tempdir().unwrap();

    let leaf = dir.path().join("leaf.cvg");
    std::fs::write(&leaf, "tempo 180\n").unwrap();

    let mid = dir.path().join("mid.cvg");
    std::fs::write(&mid, format!("include {}\n", leaf.display())).unwrap();

    let main = dir.path().join("main.cvg");
    std::fs::write(&main, format!("include {}\n", mid.display())).unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_file(&main).unwrap();
    assert!(results
        .iter()
        .any(|r| matches!(r, EvalResult::TempoChanged(180.0))));
}

/// 循環include（a→b→a）は重複スキップとして処理されエラーにならないことを検証
/// Verifies that circular includes (a→b→a) are treated as duplicate skips
///
/// 重複チェックが循環チェックより先に行われるため、同一ファイルへの再インクルードは
/// IncludeSkipped として処理される。
/// Because duplicate check is performed before cycle detection, re-including the same
/// file results in IncludeSkipped rather than CircularInclude.
#[test]
fn include_circular_detection() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.cvg");
    let b = dir.path().join("b.cvg");

    std::fs::write(&a, format!("include {}\n", b.display())).unwrap();
    std::fs::write(&b, format!("include {}\n", a.display())).unwrap();

    let mut ev = Evaluator::new(120.0);
    let result = ev.eval_file(&a);
    assert!(result.is_ok());
    let results = result.unwrap();
    assert!(results
        .iter()
        .any(|r| matches!(r, EvalResult::IncludeSkipped { .. })));
}

/// 存在しないファイルのincludeがエラーになることを検証
/// Verifies that including a nonexistent file results in an error
#[test]
fn include_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.cvg");
    std::fs::write(&main, "include nonexistent.cvg\n").unwrap();

    let mut ev = Evaluator::new(120.0);
    let result = ev.eval_file(&main);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EngineError::IncludeNotFound(_)
    ));
}

/// include先のパースエラーが伝播することを検証
/// Verifies that parse errors from included files are propagated
#[test]
fn include_parse_error_propagates() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.cvg");
    std::fs::write(&bad, "invalid !@# syntax\n").unwrap();

    let main = dir.path().join("main.cvg");
    std::fs::write(&main, format!("include {}\n", bad.display())).unwrap();

    let mut ev = Evaluator::new(120.0);
    let result = ev.eval_file(&main);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), EngineError::ParseError(_)));
}

/// 複数のincludeが順序通り展開されることを検証
/// Verifies that multiple includes are expanded in order
#[test]
fn include_multiple_files_in_order() {
    let dir = tempfile::tempdir().unwrap();

    let first = dir.path().join("first.cvg");
    std::fs::write(&first, "tempo 100\n").unwrap();

    let second = dir.path().join("second.cvg");
    std::fs::write(&second, "tempo 200\n").unwrap();

    let main = dir.path().join("main.cvg");
    std::fs::write(
        &main,
        format!(
            "include {}\ninclude {}\n",
            first.display(),
            second.display()
        ),
    )
    .unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_file(&main).unwrap();
    // 最後のtempoが適用されている
    assert!((ev.bpm() - 200.0).abs() < f64::EPSILON);
    // IncludeProcessedが2つ
    let include_count = results
        .iter()
        .filter(|r| matches!(r, EvalResult::IncludeProcessed { .. }))
        .count();
    assert_eq!(include_count, 2);
}

/// ダイヤモンド型include（A→B→shared, A→C→shared）でsharedが1回だけ処理されることを検証
/// Verifies that diamond-shaped includes (A→B→shared, A→C→shared) process shared only once
#[test]
fn include_diamond_dedup() {
    let dir = tempfile::tempdir().unwrap();

    // shared.cvg: 共通ファイル
    let shared = dir.path().join("shared.cvg");
    std::fs::write(&shared, "tempo 160\n").unwrap();

    // b.cvg: shared を include
    let b = dir.path().join("b.cvg");
    std::fs::write(&b, format!("include {}\n", shared.display())).unwrap();

    // c.cvg: shared を include
    let c = dir.path().join("c.cvg");
    std::fs::write(&c, format!("include {}\n", shared.display())).unwrap();

    // main.cvg: b と c を include（shared はダイヤモンド型で重複）
    let main = dir.path().join("main.cvg");
    std::fs::write(
        &main,
        format!("include {}\ninclude {}\n", b.display(), c.display()),
    )
    .unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_file(&main).unwrap();

    // TempoChanged は1回だけ（shared が1回だけ処理される）
    // TempoChanged appears only once (shared is processed only once)
    let tempo_count = results
        .iter()
        .filter(|r| matches!(r, EvalResult::TempoChanged(_)))
        .count();
    assert_eq!(tempo_count, 1);

    // IncludeSkipped が1回（c.cvg 経由の shared がスキップ）
    // IncludeSkipped appears once (shared via c.cvg is skipped)
    let skipped_count = results
        .iter()
        .filter(|r| matches!(r, EvalResult::IncludeSkipped { .. }))
        .count();
    assert_eq!(skipped_count, 1);

    // BPM は 160 に更新されている
    assert!((ev.bpm() - 160.0).abs() < f64::EPSILON);
}

/// 同一ファイルを直接2回includeした場合に2回目がスキップされることを検証
/// Verifies that including the same file twice directly skips the second occurrence
#[test]
fn include_direct_duplicate_skipped() {
    let dir = tempfile::tempdir().unwrap();

    let common = dir.path().join("common.cvg");
    std::fs::write(&common, "tempo 150\n").unwrap();

    // main.cvg: common を2回 include
    let main = dir.path().join("main.cvg");
    std::fs::write(
        &main,
        format!(
            "include {}\ninclude {}\n",
            common.display(),
            common.display()
        ),
    )
    .unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_file(&main).unwrap();

    // TempoChanged は1回だけ
    let tempo_count = results
        .iter()
        .filter(|r| matches!(r, EvalResult::TempoChanged(_)))
        .count();
    assert_eq!(tempo_count, 1);

    // IncludeProcessed が1回、IncludeSkipped が1回
    let processed_count = results
        .iter()
        .filter(|r| matches!(r, EvalResult::IncludeProcessed { .. }))
        .count();
    let skipped_count = results
        .iter()
        .filter(|r| matches!(r, EvalResult::IncludeSkipped { .. }))
        .count();
    assert_eq!(processed_count, 1);
    assert_eq!(skipped_count, 1);
}
