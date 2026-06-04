//! Score IR → lcvgc DSL 文字列の emitter。
//!
//! MVP の出力ルール:
//! 1. ヘッダコメント (生成元の表示)
//! 2. `device gen_device { port GEN_PORT }`
//! 3. Drum トラックがあれば `kit gen_kit { ... }`
//! 4. 各 Melodic トラック毎に `instrument <name> { device gen_device, channel N }`
//! 5. 各トラック毎に `clip <name> { ... }` を 1 つ生成 (`[bars N]` 自動算出)
//! 6. `tempo N`
//! 7. すべての clip を含む `scene gen_scene { ... }`
//! 8. `play gen_scene`
//!
//! 同時発音 (同 tick の複数ノート) は `[a b c]:dur` の和音記法でまとめる。
//! 音価は 16 分音符グリッドに量子化し、不要なものはタイ (`x:8 x:16`) で繋ぐ。
//! `LoopBlock` は `(...)*count` で展開。
//!
//! Emits lcvgc DSL from a `Score`. The MVP output structure is described
//! above. Notes are quantized onto a 16th-note grid; loop blocks become
//! `(...)*count`; drum tracks are emitted with the step-sequencer notation.

use super::quantize::quantize_ticks;
use super::score::{Event, Score, Track, TrackKind};
use super::GenOptions;
use super::GeneratorError;

/// 省略記法のキャリーオーバー状態 (emitter 用)。
///
/// parser 側の [`crate::parser::clip_shorthand::CarryOverState`] と同じ意味論を
/// 出力側で再現する。直前に書き出したオクターブ・音長を覚えておき、同じ値なら
/// 省略する。初期値は parser のデフォルトと一致させる (octave=4, duration="4")。
///
/// Carry-over state for shorthand emission. Mirrors the parser's
/// `CarryOverState` so the emitted text round-trips to the same notes.
struct EmitCarry {
    /// 直前に書き出したオクターブ。`note:oct` の oct と一致すれば省略する。
    /// Last emitted octave; omitted when the next note matches it.
    octave: u8,
    /// 直前に書き出した音長トークン文字列 (例 `"4"`, `"8."`)。
    /// 一致すれば省略する。
    /// Last emitted duration token (e.g. "4", "8."); omitted when matching.
    duration: String,
}

impl EmitCarry {
    /// parser のデフォルト (octave=4, duration=4 分音符) で初期化する。
    /// Initializes with the parser defaults (octave 4, quarter note).
    fn new() -> Self {
        EmitCarry {
            octave: 4,
            duration: "4".to_string(),
        }
    }

    /// 「どの実値とも一致しない」状態で初期化する (ループ先頭強制明示用)。
    ///
    /// `octave` を有効範囲外 (音名で表せない 255) に、`duration` を空文字に設定し、
    /// 次に書き出す音符・休符・コードが必ず oct/dur を明示するようにする。
    /// 案C のループ先頭 force を実現するためのコンストラクタ。
    ///
    /// Creates a state guaranteed not to match any real octave/duration, forcing
    /// the next emitted token to spell out its octave/duration (loop-head force).
    fn unreachable() -> Self {
        EmitCarry {
            octave: u8::MAX,
            duration: String::new(),
        }
    }

    /// 単音の省略形 `name[:oct][:dur][.]` を構築し、状態を更新して返す。
    ///
    /// # 引数 / Arguments
    /// * `name` - 音名文字列 (例 `"c"`, `"f#"`)
    /// * `oct` - オクターブ値
    /// * `dur_tok` - 音長トークン文字列 (付点付きなら末尾に `.`、例 `"8."`)
    /// * `force_oct` - `true` なら oct を必ず明示する (ループ先頭用)
    /// * `force_dur` - `true` なら dur を必ず明示する (ループ先頭用)
    ///
    /// # 戻り値 / Returns
    /// 省略適用後の DSL トークン文字列
    ///
    /// # 言語制約 / Constraint
    /// 付点付き (`dur_tok` が `.` を含む) かつ oct/dur 両方を省略すると `c.` の
    /// ような構文になりパースできない (付点は `:oct:dur` のコロン部の後でのみ
    /// 消費される)。そのため付点付きで oct も省略する場合は dur を必ず明示する。
    fn note_token(
        &mut self,
        name: &str,
        oct: u8,
        dur_tok: &str,
        force_oct: bool,
        force_dur: bool,
    ) -> String {
        let dotted = dur_tok.ends_with('.');
        let out_oct = force_oct || oct != self.octave;
        // 付点は要素固有でキャリーされない。dur を省略すると付点も書けず音長が
        // 変わってしまうため、付点付きは常に dur を明示する。
        let out_dur = force_dur || dotted || dur_tok != self.duration;
        // 状態更新は実際の値で行う (出力可否に関わらずキャリーは進む)。
        self.octave = oct;
        self.duration = dur_tok.to_string();

        let mut s = name.to_string();
        match (out_oct, out_dur) {
            (true, true) => {
                s.push(':');
                s.push_str(&oct.to_string());
                s.push(':');
                s.push_str(dur_tok);
            }
            (true, false) => {
                // 非付点で dur 一致 → oct のみ明示 (`c:6`)。
                s.push(':');
                s.push_str(&oct.to_string());
            }
            (false, true) => {
                // oct 省略 + dur 明示 (`c::8` / 付点なら `c::8.`)。
                s.push_str("::");
                s.push_str(dur_tok);
            }
            (false, false) => {
                // name のみ (oct/dur 両省略, 非付点)。
            }
        }
        s
    }

    /// 休符の省略形 `r[:dur]` を構築し、状態を更新して返す。
    ///
    /// 休符はオクターブをキャリーしない (parser の `resolve_duration_only` と同じ)。
    /// 付点付き休符 `r.` はパース不可のため、付点付きなら dur を必ず明示する。
    ///
    /// # 引数 / Arguments
    /// * `dur_tok` - 音長トークン文字列
    /// * `force_dur` - `true` なら dur を必ず明示する
    fn rest_token(&mut self, dur_tok: &str, force_dur: bool) -> String {
        let dotted = dur_tok.ends_with('.');
        // 付点休符 `r.` はパース不可のため、付点付きは常に dur を明示する。
        let out_dur = force_dur || dotted || dur_tok != self.duration;
        self.duration = dur_tok.to_string();
        if out_dur {
            format!("r:{}", dur_tok)
        } else {
            "r".to_string()
        }
    }

    /// コード括弧の省略形 `[..][:dur]` を構築し、状態を更新して返す。
    ///
    /// コードはオクターブを **外へ** キャリーしない (`self.octave` は変更しない)。
    /// 音長のみキャリーする。付点付き `[..].` はパース可能なので dur 省略可。
    ///
    /// 和音内の各音のオクターブは、parser/compiler の挙動に合わせて省略する:
    /// 和音内で oct を省略した音は **和音突入時点の `self.octave`** にフォール
    /// バックする (compiler の `oct_opt.unwrap_or(carry.octave)`)。そのため
    /// `self.octave` と一致する音だけ oct を省略でき、異なる音は明示する。
    /// 和音内で oct を書いても `self.octave` は進まないため、判定基準は常に
    /// 和音突入時の値で一定。
    ///
    /// # 引数 / Arguments
    /// * `notes` - 構成音の `(音名, オクターブ)` 列 (記譜順)
    /// * `dur_tok` - 音長トークン文字列
    /// * `force_dur` - `true` なら dur を必ず明示する
    fn chord_token(&mut self, notes: &[(&str, u8)], dur_tok: &str, force_dur: bool) -> String {
        // 和音突入時の基準オクターブ (これに一致する音だけ oct 省略可)。
        let base_oct = self.octave;
        let inner: Vec<String> = notes
            .iter()
            .map(|&(name, oct)| {
                if oct == base_oct {
                    name.to_string()
                } else {
                    format!("{}:{}", name, oct)
                }
            })
            .collect();
        let inner = inner.join(" ");

        let dotted = dur_tok.ends_with('.');
        let out_dur = force_dur || dur_tok != self.duration;
        // dur のみキャリー (oct はキャリーしないので self.octave は据え置き)。
        self.duration = dur_tok.to_string();
        if out_dur {
            format!("[{}]:{}", inner, dur_tok)
        } else if dotted {
            // dur は前回と同じだが付点だけ付ける (`[..].` は合法)。
            format!("[{}].", inner)
        } else {
            format!("[{}]", inner)
        }
    }
}

/// Score を lcvgc DSL 文字列に変換する。
///
/// Converts a Score into an lcvgc DSL string.
///
/// # Errors
/// 表現不能な構造を含む場合 `GeneratorError::Emit` を返す (現在の MVP では
/// 殆ど発生しない)。
pub fn emit(score: &Score, opts: &GenOptions) -> Result<String, GeneratorError> {
    // オプション由来の正規化を施した作業用 Score を作る。
    // - オクターブシフト: 音程 (Melodic) トラックのノートのみ ±12*n する。
    //   ドラムトラックは対象外。範囲外 (0-127) は飽和させる。
    // Build a working score with option-driven normalization applied:
    //  - octave shift affects only Melodic-track notes (drums untouched),
    //    clamping to the MIDI range.
    let mut score = apply_octave_shift_to_score(score, opts.octave_shift);
    // 音程トラックの instrument 名を fm / bass 系に正規化する。
    // Normalize melodic track names into the fm / bass family.
    normalize_instrument_names(&mut score, opts);
    let score = &score;

    let mut out = String::new();
    emit_header(score, &mut out);
    emit_device(&mut out);
    // ドラム kit (tr808) はユーザー環境側に定義がある前提とし、生成しない。
    // drum clip では `use tr808` のみを書く。
    // The tr808 kit is assumed to be defined in the user's environment; we only
    // emit `use tr808` in drum clips and do not generate the kit block.
    emit_instruments(score, &mut out);
    emit_clips(score, &mut out, opts)?;
    emit_tempo(score, &mut out);
    emit_scene(score, &mut out);
    emit_play(&mut out);
    Ok(out)
}

/// 音程 (Melodic) トラックのノートに対してのみオクターブシフトを適用した
/// Score の複製を返す。`shift == 0` のときはクローンのみで内容は不変。
/// シフト後のノート番号が MIDI 範囲 (0-127) を外れる場合は端に飽和させる。
/// ドラム (Drum) トラックは変更しない。
///
/// Returns a clone of `score` with an octave shift applied to Melodic-track
/// notes only. Notes are clamped to 0..=127. Drum tracks are left unchanged.
///
/// # 引数 / Arguments
/// * `score` - 元の Score / source score
/// * `octave_shift` - シフトするオクターブ数（正で上、負で下）/ octave shift count
///
/// # 戻り値 / Returns
/// シフト適用済みの Score / a shifted copy of the score
fn apply_octave_shift_to_score(score: &Score, octave_shift: i8) -> Score {
    let mut score = score.clone();
    if octave_shift == 0 {
        return score;
    }
    let delta = octave_shift as i32 * 12;
    for track in score.tracks.iter_mut() {
        if track.kind != TrackKind::Melodic {
            continue;
        }
        shift_events(&mut track.events, delta);
    }
    score
}

/// イベント列内の全ノートの MIDI ノート番号に `delta` を加算し 0..=127 に飽和。
/// `LoopBlock` 内のノートも再帰的にシフトする。
///
/// Adds `delta` to every note's MIDI number (clamped to 0..=127), recursing
/// into `LoopBlock`s.
fn shift_events(events: &mut [Event], delta: i32) {
    for ev in events.iter_mut() {
        match ev {
            Event::Note { midi_note, .. } => {
                *midi_note = (*midi_note as i32 + delta).clamp(0, 127) as u8;
            }
            Event::LoopBlock { events, .. } => shift_events(events, delta),
        }
    }
}

/// 音程 (Melodic) トラックの instrument 名を `fm` / `bass` 系に正規化する。
///
/// 各音程トラックの平均 MIDI ノートを求め、`opts.bass_max_avg_note` 未満なら
/// ベースとみなして `bass` 系、それ以外は `fm` 系の名前を割り当てる。同種が複数
/// ある場合は 2 つ目以降に通し番号を付け (`fm`, `fm2`, `fm3` / `bass`, `bass2`)、
/// clip 名や scene 参照が衝突しないようユニークにする。ドラムトラックは変更しない。
///
/// Normalizes melodic-track instrument names into the `fm` / `bass` family.
/// A track whose mean MIDI note is below `opts.bass_max_avg_note` is treated as
/// a bass line. Duplicates get a numeric suffix to stay unique. Drum tracks are
/// left untouched.
///
/// # 引数 / Arguments
/// * `score` - 正規化対象の Score (in-place 変更) / score to mutate in place
/// * `opts` - ベース判定しきい値を含む生成オプション / generator options
fn normalize_instrument_names(score: &mut Score, opts: &GenOptions) {
    let mut fm_count = 0u32;
    let mut bass_count = 0u32;
    let mut drum_count = 0u32;
    for track in score.tracks.iter_mut() {
        match track.kind {
            TrackKind::Melodic => {
                let is_bass = mean_note(&track.events)
                    .map(|avg| avg < opts.bass_max_avg_note as f32)
                    .unwrap_or(false);
                track.name = if is_bass {
                    bass_count += 1;
                    numbered("bass", bass_count)
                } else {
                    fm_count += 1;
                    numbered("fm", fm_count)
                };
            }
            TrackKind::Drum => {
                // ドラムトラックの clip 名は `drums`（複数なら drums2, ...）。
                drum_count += 1;
                track.name = numbered("drums", drum_count);
            }
        }
    }
}

/// `base` と通し番号から名前を作る。1 番目は番号を付けず `base` のまま。
/// 2 番目以降は `base2`, `base3`, ... とする。
///
/// Builds `base`, `base2`, `base3`, ... (no suffix for the first).
fn numbered(base: &str, n: u32) -> String {
    if n <= 1 {
        base.to_string()
    } else {
        format!("{}{}", base, n)
    }
}

/// イベント列の全ノートの平均 MIDI ノート番号を返す。ノートが無ければ `None`。
/// `LoopBlock` 内のノートも含める。
///
/// Returns the mean MIDI note over all notes (including those inside loop
/// blocks), or `None` when the track has no notes.
fn mean_note(events: &[Event]) -> Option<f32> {
    let notes = iter_notes(events);
    if notes.is_empty() {
        return None;
    }
    let sum: u32 = notes.iter().map(|&(_, _, m, _)| m as u32).sum();
    Some(sum as f32 / notes.len() as f32)
}

/// 生成元情報のヘッダコメントを書き出す。
/// Writes the header comment.
fn emit_header(score: &Score, out: &mut String) {
    out.push_str("// generated by lcvgc-gen\n");
    if let Some(title) = &score.title {
        out.push_str("// title: ");
        out.push_str(title);
        out.push('\n');
    }
    out.push_str(&format!(
        "// time signature: {}/{}\n",
        score.time_signature.numerator, score.time_signature.denominator
    ));
    out.push('\n');
}

/// `device gen_device { port GEN_PORT }` を書き出す。
///
/// ユーザは生成された DSL の `port` を環境に合わせて書き換える前提。
/// Writes the placeholder device block.
fn emit_device(out: &mut String) {
    out.push_str("device gen_device {\n  port GEN_PORT\n}\n\n");
}

/// 各 Melodic トラックに対し instrument を書き出す。
/// Writes one instrument per melodic track.
fn emit_instruments(score: &Score, out: &mut String) {
    for t in &score.tracks {
        if t.kind != TrackKind::Melodic {
            continue;
        }
        out.push_str(&format!(
            "instrument {} {{\n  device gen_device\n  channel {}\n}}\n\n",
            t.name, t.midi_channel
        ));
    }
}

/// 各トラックを `clip` として書き出す。
/// Writes one clip per track.
fn emit_clips(score: &Score, out: &mut String, opts: &GenOptions) -> Result<(), GeneratorError> {
    for t in &score.tracks {
        let bars = bars_for_track(t, score);
        let clip_name = clip_name_for(t);
        match t.kind {
            TrackKind::Melodic => {
                emit_melodic_clip(&clip_name, t, score, bars, out, opts.bars_per_marker)?;
            }
            TrackKind::Drum => {
                emit_drum_clip(&clip_name, t, score, bars, out)?;
            }
        }
    }
    Ok(())
}

/// Melodic / Drum 共通で「8 小節ごとに改行を入れる」しきい値。
/// ループブロック内部の再帰呼び出しでは改行を入れたくないので、
/// 外側からのみ `Some(BARS_PER_WRAP)` を渡し、内側は `None` を渡す。
const BARS_PER_WRAP: u32 = 8;

/// 小節先頭トークンの桁位置と小節番号を保持する。
///
/// `emit_event_sequence` が本体トークン列を `out` (clip body バッファ) へ
/// 書き出す過程で、各小節の先頭トークンの「body バッファ内の絶対バイト
/// オフセット」と「小節番号 (1 始まり)」を記録する。後段の
/// [`build_bar_comment_lines`] が、これを物理行ごとの行内桁へ変換して
/// 小節番号コメント行を組み立てる。
///
/// Records a bar-head token's byte offset within the clip body and its
/// (1-based) bar number, used to build aligned bar-number comment lines.
#[derive(Debug, Clone, Copy)]
struct BarMarker {
    /// clip body バッファ内の絶対バイトオフセット (トークン本体の先頭桁)。
    /// Absolute byte offset of the token within the clip-body buffer.
    abs_col: usize,
    /// 小節番号 (1 始まり)。
    /// 1-based bar number.
    bar_no: u32,
}

/// 1 つの Melodic clip を書き出す。
///
/// 同時発音 (同 start_tick) のノートは `[a b c]:dur` に集約する。
/// `LoopBlock` は `(...)*count` で展開する。
/// 8 小節境界では改行 + インデントを挿入し、可読性を保つ。
///
/// Emits a single melodic clip. Wraps every 8 bars for readability.
fn emit_melodic_clip(
    clip_name: &str,
    track: &Track,
    score: &Score,
    bars: u32,
    out: &mut String,
    bars_per_marker: u32,
) -> Result<(), GeneratorError> {
    out.push_str(&format!("clip {} [bars {}] {{\n", clip_name, bars));
    let mut cursor: u64 = 0;
    let mut body = String::new();
    body.push_str(&format!("  {}", track.name));
    // clip 単位でキャリー状態を初期化する (parser のデフォルトに合わせる)。
    let mut carry = EmitCarry::new();
    // 小節先頭トークンの桁位置を集める。
    let mut markers: Vec<BarMarker> = Vec::new();
    emit_event_sequence(
        &track.events,
        score,
        &mut cursor,
        &mut body,
        Some(BARS_PER_WRAP),
        &mut carry,
        &mut markers,
    )?;
    body.push('\n');
    // 小節番号コメント行を本体行の直下に差し込む。
    let body = interleave_bar_comments(&body, &markers, bars_per_marker);
    out.push_str(&body);
    out.push_str("}\n\n");
    Ok(())
}

/// clip body に小節番号コメント行を差し込んだ文字列を返す。
///
/// `body` を物理行 (`\n` 区切り) に分割し、各本体行に属する [`BarMarker`] から
/// 行内桁を算出して `// ...N...` 形式のコメント行を組み立て、本体行の直後に
/// 挿入する。`bars_per_marker` の倍数に当たる小節のみを対象とし、先頭小節 (1)
/// は常に省略する。`bars_per_marker == 0` のときはコメント行を一切挿入しない。
///
/// # 引数 / Arguments
/// * `body` - clip body 文字列 (末尾 `\n` 込み)
/// * `markers` - 小節先頭トークンの絶対バイトオフセットと小節番号
/// * `bars_per_marker` - 何小節ごとにマーカーを出すか (0 で無効)
///
/// # 戻り値 / Returns
/// コメント行を差し込んだ body 文字列
fn interleave_bar_comments(body: &str, markers: &[BarMarker], bars_per_marker: u32) -> String {
    if bars_per_marker == 0 || markers.is_empty() {
        return body.to_string();
    }
    // 出力対象の小節のみ残す。先頭小節 (bar_no == 1) は省略。
    // bars_per_marker == N のとき、(bar_no - 1) % N == 0 の小節を出す
    // (N=1 → 全小節, N=2 → 1,3,5,... のうち 1 を除いた 3,5,...)。
    let target =
        |bar_no: u32| -> bool { bar_no >= 2 && (bar_no - 1).is_multiple_of(bars_per_marker) };

    // 各物理行の [start, end) バイト範囲を求める。end は改行を含まない。
    // body は "  lead ...\n   ...\n}" のように行が連なる。
    let mut result = String::with_capacity(body.len() + body.len() / 4);
    let mut line_start = 0usize; // 現在行の body 内開始オフセット
    let bytes = body.as_bytes();
    let mut idx = 0usize;
    while idx <= body.len() {
        let at_newline = idx < body.len() && bytes[idx] == b'\n';
        let at_end = idx == body.len();
        if at_newline || at_end {
            let line = &body[line_start..idx];
            result.push_str(line);
            if at_newline {
                result.push('\n');
            }
            // この行に属するマーカーを集めてコメント行を作る。
            let comment =
                build_bar_comment_line(line, line_start, line_start..idx, markers, &target);
            if let Some(c) = comment {
                result.push_str(&c);
                result.push('\n');
            }
            line_start = idx + 1;
        }
        idx += 1;
    }
    result
}

/// 1 本の本体行に対する小節番号コメント行を組み立てる。
///
/// `markers` のうち、行の絶対バイト範囲 `range` に収まり、かつ `target` を
/// 満たす小節について、行頭からの桁位置に小節番号を配置した `// ...` 文字列を
/// 返す。該当マーカーが無ければ `None`。
///
/// # 引数 / Arguments
/// * `line` - 本体行文字列 (改行を含まない)
/// * `line_start` - `line` の body 内開始バイトオフセット
/// * `range` - `line` の body 内バイト範囲 `[start, end)`
/// * `markers` - 全 [`BarMarker`]
/// * `target` - その小節番号を出力対象とするか判定するクロージャ
///
/// # 戻り値 / Returns
/// コメント行文字列 (改行なし)。該当無しなら `None`。
fn build_bar_comment_line(
    _line: &str,
    line_start: usize,
    range: std::ops::Range<usize>,
    markers: &[BarMarker],
    target: &impl Fn(u32) -> bool,
) -> Option<String> {
    // (行内桁, 小節番号) を桁昇順で集める。
    let mut placements: Vec<(usize, u32)> = markers
        .iter()
        .filter(|m| range.contains(&m.abs_col) && target(m.bar_no))
        .map(|m| (m.abs_col - line_start, m.bar_no))
        .collect();
    if placements.is_empty() {
        return None;
    }
    placements.sort_by_key(|(col, _)| *col);

    // "//" で始め、各小節番号をその桁位置に空白パディングで配置する。
    // `col >= s.len()` ならちょうど目的の桁に数字を置けるよう空白で埋める
    // (col == s.len() のときは埋める空白 0 個でそのまま置ける)。直前の数字が
    // 長く目的桁を追い越している場合 (col < s.len()) のみ、空白 1 つで区切って
    // 続ける (桁ズレより可読性を優先)。
    let mut s = String::from("//");
    for (col, bar_no) in placements {
        let num = bar_no.to_string();
        if col >= s.len() {
            // 桁位置まで空白で埋める (col == s.len() なら 0 個)。
            s.push_str(&" ".repeat(col - s.len()));
        } else {
            // 既に追い越している場合は空白 1 つで区切る。
            s.push(' ');
        }
        s.push_str(&num);
    }
    Some(s)
}

/// 1 つの Drum clip を書き出す。
///
/// `resolution 16` 固定で、16 分音符グリッドにヒットを配置する。
///
/// Emits a single drum clip with `resolution 16`.
fn emit_drum_clip(
    clip_name: &str,
    track: &Track,
    score: &Score,
    bars: u32,
    out: &mut String,
) -> Result<(), GeneratorError> {
    let steps_per_bar: u32 = 16; // resolution 16 + 4/4 を前提
    let total_steps = (steps_per_bar * bars) as usize;
    let ticks_per_step = (score.ppq / 4).max(1) as u64;

    // (midi_note, [step1, step2, ...]) のマップを作る。
    let mut per_note: std::collections::BTreeMap<u8, Vec<char>> = std::collections::BTreeMap::new();
    for (start, _end, midi_note, velocity) in iter_notes(&track.events) {
        let step = (start / ticks_per_step) as usize;
        if step >= total_steps {
            continue;
        }
        let row = per_note
            .entry(midi_note)
            .or_insert_with(|| vec!['.'; total_steps]);
        row[step] = velocity_to_hit_symbol(velocity);
    }

    out.push_str(&format!("clip {} [bars {}] {{\n", clip_name, bars));
    out.push_str("  use tr808\n");
    out.push_str("  resolution 16\n");
    let steps_per_wrap = (steps_per_bar * BARS_PER_WRAP) as usize;
    for (midi_note, row) in per_note {
        if let Some(label) = drum_label(midi_note) {
            let row_str: String = row.into_iter().collect();
            out.push_str(&format!(
                "  {} {}\n",
                label,
                wrap_drum_row(&row_str, steps_per_wrap)
            ));
        }
    }
    out.push_str("}\n\n");
    Ok(())
}

/// drum row 文字列を `chunk_size` ごとに `\` + 改行 + インデントで折り返す。
///
/// 長さが `chunk_size` 以下ならそのまま返す。それ以上なら、最初のチャンクの
/// 末尾に ` \\\n      ` を挿入し、残りを再帰的に折り返す。
/// 末尾のチャンクには `\` を付けない。
///
/// Wraps a drum-row pattern string every `chunk_size` characters using a
/// backslash continuation that the parser accepts as a single logical row.
fn wrap_drum_row(row: &str, chunk_size: usize) -> String {
    if chunk_size == 0 || row.len() <= chunk_size {
        return row.to_string();
    }
    let mut out = String::with_capacity(row.len() + row.len() / chunk_size * 9);
    let mut remaining = row;
    let mut first = true;
    while !remaining.is_empty() {
        if !first {
            // 継続行のインデント (8 spaces — drum row 内のステップ列と視覚的に揃える)
            out.push_str("      ");
        }
        let take = remaining.len().min(chunk_size);
        out.push_str(&remaining[..take]);
        remaining = &remaining[take..];
        if !remaining.is_empty() {
            out.push_str(" \\\n");
        }
        first = false;
    }
    out
}

/// 量子化したヒット symbol を返す。
///
/// `velocity >= 110` → アクセント (`X`)
/// `velocity <= 60`  → ゴースト (`o`)
/// それ以外         → 通常 (`x`)
fn velocity_to_hit_symbol(velocity: u8) -> char {
    if velocity >= 110 {
        'X'
    } else if velocity <= 60 {
        'o'
    } else {
        'x'
    }
}

/// Melodic イベント列を文字列化して body に追記する。
///
/// イベントは `start_tick` 昇順でソートしたうえで、同 tick のノートは和音
/// (`[a b c]`) としてまとめる。`LoopBlock` は `(...)*count` で展開し、内部の
/// 並びにも同じロジックを再帰適用する。
///
/// `wrap_every_n_bars` が `Some(n)` のとき、n 小節境界をまたぐ前に改行と
/// インデントを挿入する。`LoopBlock` 内部の再帰では `None` を渡し、
/// `(...)*N` 表記内に改行を入れない。
///
/// `markers` には、各小節先頭の本体トークンの「`out` 内絶対バイトオフセット」と
/// 小節番号 (1 始まり) を追記する。`LoopBlock` 内部では `inner_out` への相対桁で
/// 集め、呼び出し側が親 `out` への連結オフセットを加算して取り込む。これにより
/// 後段で桁揃えした小節番号コメント行を組み立てられる。
///
/// Serializes a sequence of melodic events into the clip body. When
/// `wrap_every_n_bars` is `Some(n)`, inserts a newline + indent each time the
/// bar cursor crosses an n-bar boundary. Pass `None` for recursive
/// LoopBlock expansion so the `(...)*N` payload stays on a single line.
/// `markers` accumulates each bar-head token's byte offset within `out` and its
/// 1-based bar number, used later to build aligned bar-number comment lines.
fn emit_event_sequence(
    events: &[Event],
    score: &Score,
    cursor: &mut u64,
    out: &mut String,
    wrap_every_n_bars: Option<u32>,
    carry: &mut EmitCarry,
    markers: &mut Vec<BarMarker>,
) -> Result<(), GeneratorError> {
    // 1) ノートと LoopBlock を時系列で取り出す
    let mut items: Vec<&Event> = events.iter().collect();
    items.sort_by_key(|e| e.start_tick());

    let bar_t = bar_ticks(score);

    // ある tick が属する小節 index (0 始まり) を返す。bar_t==0 のときは None。
    let bar_index_of = |tick: u64| -> Option<u64> { tick.checked_div(bar_t) };
    // 直前に書いた本体トークンが属していた小節 index。これと異なる小節の
    // 先頭トークンを書くとき、その桁にマーカーを記録する。初期値は cursor の
    // 小節 (=この呼び出しの開始小節)。
    let mut last_bar: Option<u64> = bar_index_of(*cursor);

    // 直前のトークン書き出し時に、これから書く要素の start_tick が前回処理
    // した bar チャンクと異なれば改行 + インデントを挿入する。
    let wrap_chunk_of = |tick: u64| -> Option<u64> {
        let wrap_n = wrap_every_n_bars? as u64;
        if bar_t == 0 || wrap_n == 0 {
            return None;
        }
        Some(tick / bar_t / wrap_n)
    };
    let mut last_wrap_chunk: Option<u64> = wrap_chunk_of(*cursor);

    // 2) 同 tick のノートをまとめながら順に書き出す
    let mut i = 0;
    while i < items.len() {
        let elem_tick = items[i].start_tick();
        // 8 小節境界をまたいだら改行 + インデントを挿入
        if let (Some(prev), Some(cur)) = (last_wrap_chunk, wrap_chunk_of(elem_tick)) {
            if cur > prev {
                out.push_str("\n   ");
                last_wrap_chunk = Some(cur);
            }
        }
        match items[i] {
            Event::Note { start_tick, .. } => {
                // 休符で隙間を埋める
                if *start_tick > *cursor {
                    let gap = *start_tick - *cursor;
                    write_rest(out, gap, score.ppq, carry);
                    *cursor = *start_tick;
                }
                // 本体トークン書き出し直前: 小節先頭ならマーカーを記録する。
                // write_chord_or_note は先頭にスペースを 1 つ書くため、その分
                // (+1) を桁に加えてトークン本体の開始桁に合わせる。
                if let Some(cur_bar) = bar_index_of(*start_tick) {
                    let is_new_bar = last_bar.is_none_or(|prev| cur_bar > prev);
                    if is_new_bar {
                        markers.push(BarMarker {
                            abs_col: out.len() + 1,       // 先頭スペース分 +1
                            bar_no: (cur_bar + 1) as u32, // 0 始まり → 1 始まり
                        });
                    }
                    last_bar = Some(cur_bar);
                }
                // 同 tick のノートを集約 (和音検出)
                let mut chord: Vec<(u8, u64)> = Vec::new(); // (midi, end_tick)
                while i < items.len() {
                    match items[i] {
                        Event::Note {
                            start_tick: s,
                            end_tick,
                            midi_note,
                            ..
                        } if *s == *start_tick => {
                            chord.push((*midi_note, *end_tick));
                            i += 1;
                        }
                        _ => break,
                    }
                }
                let chord_end = chord.iter().map(|(_, e)| *e).max().unwrap_or(*start_tick);
                let dur_ticks = chord_end.saturating_sub(*start_tick);
                write_chord_or_note(out, &chord, dur_ticks, score.ppq, carry);
                *cursor = chord_end;
            }
            Event::LoopBlock {
                start_tick,
                events: inner,
                count,
            } => {
                // ループブロック先頭まで休符で進める
                if *start_tick > *cursor {
                    let gap = *start_tick - *cursor;
                    write_rest(out, gap, score.ppq, carry);
                    *cursor = *start_tick;
                }
                // 内部イベントを文字列化して `( ... )*N` で囲む。
                // 内部では改行を入れないため None を渡す。
                //
                // 案C: ループ境界をまたぐ省略を防ぐため、ループ内では先頭要素の
                // oct/dur を必ず明示する。これを「キャリー状態を一致し得ない値に
                // リセットしてから inner を書く」ことで実現する。inner 内で最初に
                // oct を持つ音符・最初に dur を持つ要素は強制的に明示され、各ループ
                // が必ず同じ先頭状態から始まるため意味的に常に等価となる。
                // リセットした状態を inner 終了後も親へ反映する (= ループ末尾の
                // 状態がループ後の要素にキャリーされる。これは parser の挙動と一致)。
                let mut inner_out = String::new();
                let mut inner_cursor: u64 = *start_tick;
                let mut inner_carry = EmitCarry::unreachable();
                // ループ内のマーカーは「1 周目の小節番号」を inner_out への相対桁で
                // 集める。inner_cursor は実 tick で進むため、bar_index_of により
                // 1 周目の絶対小節番号 (= start_tick からの相対小節) が記録される。
                let mut inner_markers: Vec<BarMarker> = Vec::new();
                emit_event_sequence(
                    inner,
                    score,
                    &mut inner_cursor,
                    &mut inner_out,
                    None,
                    &mut inner_carry,
                    &mut inner_markers,
                )?;
                // ループ末尾のキャリー状態を親に反映する。
                *carry = inner_carry;
                let one_iter_ticks = inner_cursor.saturating_sub(*start_tick);
                // 親 out への連結。`" ("` (2 文字) の後に trim_start 済み inner を
                // 置くため、inner マーカーの相対桁 → 親絶対桁の変換は
                //   親桁 = (連結直前の out.len()) + 2 + (相対桁 - trimmed_prefix)
                // となる。trimmed_prefix は inner_out 先頭の空白数。
                let trimmed = inner_out.trim_start();
                let trimmed_prefix = inner_out.len() - trimmed.len();
                let base_col = out.len() + 2; // " (" の 2 文字分
                for m in &inner_markers {
                    // trim で削られた先頭空白より前のマーカーは存在しない想定だが、
                    // 念のため飽和で扱う。
                    let rel = m.abs_col.saturating_sub(trimmed_prefix);
                    markers.push(BarMarker {
                        abs_col: base_col + rel,
                        bar_no: m.bar_no,
                    });
                }
                out.push_str(" (");
                out.push_str(trimmed);
                out.push_str(&format!(" )*{}", count));
                *cursor += one_iter_ticks * (*count as u64);
                // ループ後の通常トークンは N 周分進んだ小節に戻る。last_bar を
                // ループ末尾の実小節 (start_tick + one_iter_ticks*N の手前) に
                // 合わせる。次トークンの bar_index_of と比較されるため、ここでは
                // cursor 直前の小節へ更新しておく。
                if let Some(b) = bar_index_of((*cursor).saturating_sub(1)) {
                    last_bar = Some(b);
                }
                i += 1;
            }
        }
    }
    Ok(())
}

/// 休符を書き出す (隙間 tick → `r:duration` 列)。
///
/// 各音長トークンは [`EmitCarry`] を介して省略形 (`r` / `r:dur`) で書き出す。
/// 休符はオクターブをキャリーしない。
fn write_rest(out: &mut String, ticks: u64, ppq: u32, carry: &mut EmitCarry) {
    let (tokens, _, _) = quantize_ticks(ticks, ppq);
    for t in tokens {
        out.push(' ');
        out.push_str(&carry.rest_token(t.as_str(), false));
    }
}

/// 単音 or 和音を書き出す。
///
/// `chord` が 1 要素なら単音 `note[:oct][:dur]`、複数なら `[a:o b:o c:o][:dur]`
/// 記法。音価は最長要素の長さで量子化し、複数 token になる場合はタイ (同音の
/// 繰り返し) で繋ぐ。各トークンは [`EmitCarry`] を介して省略形で書き出す。
///
/// 単音はオクターブ・音長をキャリーする。和音はオクターブをキャリーせず音長
/// のみキャリーする (内部の各音には oct を明示する)。
fn write_chord_or_note(
    out: &mut String,
    chord: &[(u8, u64)],
    dur_ticks: u64,
    ppq: u32,
    carry: &mut EmitCarry,
) {
    let (tokens, _, _) = quantize_ticks(dur_ticks, ppq);
    if chord.len() == 1 {
        // 単音: note_token で oct/dur を省略する。
        let (midi, _) = chord[0];
        let (n, o) = midi_to_note_name(midi);
        for tok in tokens {
            out.push(' ');
            out.push_str(&carry.note_token(n, o, tok.as_str(), false, false));
        }
    } else {
        // 和音: 内部 pitch は和音突入時のオクターブと一致する音だけ省略し、
        // dur もキャリーで省略する。
        let notes: Vec<(&str, u8)> = chord.iter().map(|(m, _)| midi_to_note_name(*m)).collect();
        for tok in tokens {
            out.push(' ');
            out.push_str(&carry.chord_token(&notes, tok.as_str(), false));
        }
    }
}

/// テンポを書き出す。
fn emit_tempo(score: &Score, out: &mut String) {
    out.push_str(&format!("tempo {}\n\n", score.initial_bpm.round() as i32));
}

/// 全 clip を含む scene を書き出す。
fn emit_scene(score: &Score, out: &mut String) {
    out.push_str("scene gen_scene {\n");
    for t in &score.tracks {
        out.push_str(&format!("  {}\n", clip_name_for(t)));
    }
    out.push_str("}\n\n");
}

/// トラックに対応する clip 名を返す。
/// 音程トラックは `<name>_clip`、ドラムトラックは `<name>`（=`drums`）。
/// emit_clips と emit_scene で同じ命名を共有するためのヘルパー。
///
/// Returns the clip name for a track: `<name>_clip` for melodic tracks,
/// `<name>` for drum tracks. Shared by emit_clips and emit_scene.
fn clip_name_for(track: &Track) -> String {
    match track.kind {
        TrackKind::Melodic => format!("{}_clip", track.name),
        TrackKind::Drum => track.name.clone(),
    }
}

/// 自動再生コマンド。
fn emit_play(out: &mut String) {
    out.push_str("play gen_scene\n");
}

/// トラックの bar 数を算出する。
///
/// `total_ticks / (numerator * ppq * 4 / denominator)` の切り上げ。最小 1 bar。
fn bars_for_track(track: &Track, score: &Score) -> u32 {
    let last = track_last_tick(&track.events);
    let bar_ticks = bar_ticks(score);
    if bar_ticks == 0 {
        return 1;
    }
    let bars = last.div_ceil(bar_ticks);
    bars.max(1) as u32
}

/// 1 bar あたりの tick 数 (`numerator * ppq * 4 / denominator`)。
fn bar_ticks(score: &Score) -> u64 {
    let num = score.time_signature.numerator as u64;
    let den = score.time_signature.denominator as u64;
    if den == 0 {
        return 0;
    }
    num * (score.ppq as u64) * 4 / den
}

/// イベント列の最終 tick (Note の end_tick or LoopBlock 末尾) を返す。
fn track_last_tick(events: &[Event]) -> u64 {
    let mut max = 0u64;
    for e in events {
        match e {
            Event::Note { end_tick, .. } => max = max.max(*end_tick),
            Event::LoopBlock {
                start_tick,
                events,
                count,
            } => {
                let one = track_last_tick(events).saturating_sub(*start_tick);
                max = max.max(*start_tick + one * (*count as u64));
            }
        }
    }
    max
}

/// MIDI ノート番号を `(音名文字列, オクターブ)` に変換する。
///
/// lcvgc は `c1`〜`c9` 等の小文字音名 + `#` を使う。オクターブは MIDI 60 = C4
/// 規約に従う。
fn midi_to_note_name(midi: u8) -> (&'static str, u8) {
    let names = [
        "c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b",
    ];
    let octave = (midi / 12).saturating_sub(1); // MIDI 60 → 4
    let name = names[(midi % 12) as usize];
    (name, octave)
}

/// GM ドラムマップから、tr808 系の楽器名ラベルへ変換する。
///
/// 主要 5 種は要望に合わせて `kick` / `snare` / `oh` (open hat) / `ch` (closed
/// hat) / `cp` (clap) に寄せる。その他 (tom / crash / ride など) は簡潔な
/// 既存ラベルを維持する。未知のノートは `None` を返す。
///
/// Maps a GM drum note to a tr808-style instrument label. The five primary
/// voices map to `kick` / `snare` / `oh` / `ch` / `cp`; others keep concise
/// labels. Unknown notes return `None`.
fn drum_label(midi: u8) -> Option<&'static str> {
    Some(match midi {
        35 | 36 => "kick",    // Acoustic/Bass Drum → kick
        37 => "rim",          // Side Stick
        38 | 40 => "snare",   // Acoustic/Electric Snare
        39 => "cp",           // Hand Clap → clap
        41 => "tom_lo_floor", // Low Floor Tom
        42 | 44 => "ch",      // Closed/Pedal Hi-Hat → closed hat
        43 => "tom_hi_floor", // High Floor Tom
        45 => "tom_lo",       // Low Tom
        46 => "oh",           // Open Hi-Hat → open hat
        47 => "tom_lo_mid",   // Low-Mid Tom
        48 => "tom_hi_mid",   // High-Mid Tom
        49 => "crash",        // Crash Cymbal 1
        50 => "tom_hi",       // High Tom
        51 => "ride",         // Ride Cymbal 1
        52 => "crash_china",  // Chinese Cymbal
        53 => "ride_bell",    // Ride Bell
        54 => "tambourine",   // Tambourine
        55 => "crash_splash", // Splash Cymbal
        56 => "cowbell",      // Cowbell
        57 => "crash2",       // Crash Cymbal 2
        58 => "vibraslap",    // Vibraslap
        59 => "ride2",        // Ride Cymbal 2
        60 => "hi_bongo",     // High Bongo
        61 => "lo_bongo",     // Low Bongo
        _ => return None,
    })
}

/// `Event::Note` だけを `(start, end, midi, velocity)` で列挙する (LoopBlock の中も再帰)。
fn iter_notes(events: &[Event]) -> Vec<(u64, u64, u8, u8)> {
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::Note {
                start_tick,
                end_tick,
                midi_note,
                velocity,
            } => out.push((*start_tick, *end_tick, *midi_note, *velocity)),
            Event::LoopBlock { events, .. } => {
                out.extend(iter_notes(events));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::score::{Event, Score, TimeSignature, Track, TrackKind};

    /// emit 結果から `clip fm_clip { ... }` の本体 (trim 済み) を取り出す。
    /// Extracts the trimmed body of the `fm_clip` block from emitted DSL.
    fn melodic_clip_body(dsl: &str) -> String {
        let start = dsl.find("clip fm_clip").expect("fm_clip 開始");
        let open = dsl[start..].find('{').unwrap() + start + 1;
        let close = dsl[open..].find('}').unwrap();
        dsl[open..open + close].trim().to_string()
    }

    fn one_note_score() -> Score {
        Score {
            ppq: 480,
            initial_bpm: 120.0,
            time_signature: TimeSignature::default(),
            title: Some("test".into()),
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events: vec![Event::Note {
                    start_tick: 0,
                    end_tick: 480, // 1 拍
                    midi_note: 60, // C4
                    velocity: 100,
                }],
            }],
        }
    }

    /// 指定した平均音域の単一ノートトラックを n 本持つ score を作る。
    /// Builds a score with `midis.len()` melodic tracks, one note each.
    fn score_with_track_notes(midis: &[u8]) -> Score {
        let tracks = midis
            .iter()
            .enumerate()
            .map(|(i, &m)| Track {
                name: format!("orig_{i}"),
                midi_channel: (i as u8) + 1,
                kind: TrackKind::Melodic,
                events: vec![Event::Note {
                    start_tick: 0,
                    end_tick: 480,
                    midi_note: m,
                    velocity: 100,
                }],
            })
            .collect();
        Score {
            ppq: 480,
            initial_bpm: 120.0,
            time_signature: TimeSignature::default(),
            title: None,
            tracks,
        }
    }

    #[test]
    fn low_track_named_bass_high_track_named_fm() {
        // C2(36)=低音域→bass、C5(72)=高音域→fm。
        let s = score_with_track_notes(&[36, 72]);
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        assert!(dsl.contains("instrument bass"), "bass がない: {dsl}");
        assert!(dsl.contains("instrument fm"), "fm がない: {dsl}");
        assert!(dsl.contains("clip bass_clip"));
        assert!(dsl.contains("clip fm_clip"));
    }

    #[test]
    fn multiple_fm_tracks_get_numeric_suffix() {
        // 高音域 3 本 → fm, fm2, fm3 とユニーク採番される。
        let s = score_with_track_notes(&[60, 64, 67]);
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        assert!(dsl.contains("instrument fm "), "fm がない: {dsl}");
        assert!(dsl.contains("instrument fm2 "), "fm2 がない: {dsl}");
        assert!(dsl.contains("instrument fm3 "), "fm3 がない: {dsl}");
    }

    #[test]
    fn bass_threshold_is_configurable() {
        // しきい値を 70 に上げれば C5(72) 未満の C4(60) も bass 扱いになる…
        // ではなく 60<70 なので bass。72>=70 は fm。
        let s = score_with_track_notes(&[60, 72]);
        let opts = GenOptions {
            bass_max_avg_note: 70,
            ..Default::default()
        };
        let dsl = emit(&s, &opts).unwrap();
        assert!(dsl.contains("instrument bass"), "bass がない: {dsl}");
        assert!(dsl.contains("instrument fm"), "fm がない: {dsl}");
    }

    #[test]
    fn octave_shift_up_raises_emitted_octave() {
        // C4 を +1 オクターブ → C5。clip 本体に `c:5` が現れる。
        let opts = GenOptions {
            octave_shift: 1,
            ..Default::default()
        };
        let dsl = emit(&one_note_score(), &opts).unwrap();
        let body = melodic_clip_body(&dsl);
        assert!(body.contains("c:5"), "body was: {}", body);
    }

    #[test]
    fn octave_shift_down_lowers_emitted_octave() {
        // C4 を -1 オクターブ → C3。
        let opts = GenOptions {
            octave_shift: -1,
            ..Default::default()
        };
        let dsl = emit(&one_note_score(), &opts).unwrap();
        let body = melodic_clip_body(&dsl);
        assert!(body.contains("c:3"), "body was: {}", body);
    }

    #[test]
    fn octave_shift_does_not_affect_drum_kit_notes() {
        // ドラムトラックの kit ノートはオクターブシフトの影響を受けない。
        let mut s = one_note_score();
        s.tracks.push(Track {
            name: "drums".into(),
            midi_channel: 10,
            kind: TrackKind::Drum,
            events: vec![Event::Note {
                start_tick: 0,
                end_tick: 120,
                midi_note: 36, // GM Bass Drum (C2)
                velocity: 100,
            }],
        });
        let before = emit(&s, &GenOptions::default()).unwrap();
        let after = emit(
            &s,
            &GenOptions {
                octave_shift: 2,
                ..Default::default()
            },
        )
        .unwrap();
        // ドラム clip (clip drums {...}) のステップ行はシフトで変わらない。
        let drum_before = before.split("clip drums").nth(1).unwrap();
        let drum_after = after.split("clip drums").nth(1).unwrap();
        assert_eq!(drum_before, drum_after, "drum steps must not shift");
        // kick の step 行が含まれること。
        assert!(after.contains("kick "), "kick row missing: {after}");
    }

    /// 各小節の先頭に 1 拍ノートを置いた `bars` 小節のメロディトラック score。
    /// tick = bar * bar_ticks にノートを置き、小節先頭の桁揃え検証に使う。
    /// Builds a `bars`-bar melodic score with one note at each bar head.
    fn score_with_one_note_per_bar(bars: u32, ppq: u32) -> Score {
        let bar_t = (ppq as u64) * 4; // 4/4 の 1 小節 tick
        let events = (0..bars as u64)
            .map(|b| Event::Note {
                start_tick: b * bar_t,
                end_tick: b * bar_t + ppq as u64, // 4 分音符
                midi_note: 60,
                velocity: 100,
            })
            .collect();
        Score {
            ppq,
            initial_bpm: 120.0,
            time_signature: TimeSignature::default(),
            title: None,
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events,
            }],
        }
    }

    /// clip 本体 (melodic_clip_body の結果) から `//` で始まるコメント行を
    /// 出現順に取り出す。
    /// Extracts the bar-number comment lines (those starting with `//`).
    fn comment_lines(body: &str) -> Vec<&str> {
        body.lines()
            .map(|l| l.trim_end())
            .filter(|l| l.trim_start().starts_with("//"))
            .collect()
    }

    /// emit 結果全体について「各コメント行の小節番号の開始桁が、直前の本体行で
    /// 非空白 (トークン本体の開始) になっている」ことを検証し、桁ズレ件数を返す。
    /// 数字が隣接して並ぶ密なケースの桁ズレ回帰検出に使う。
    /// Returns the number of bar-number digits whose column does not line up with
    /// a non-space token-head character on the preceding body line.
    fn count_column_misalignments(dsl: &str) -> usize {
        let mut prev: Option<&str> = None;
        let mut errors = 0;
        for line in dsl.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                let Some(body_line) = prev else { continue };
                // コメント行の各数字の開始桁を取り、本体行の同桁が非空白か確認。
                let bytes = line.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i].is_ascii_digit() {
                        let col = i;
                        // 数字列の終端まで進める。
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        let ch = body_line.as_bytes().get(col).copied().unwrap_or(b' ');
                        if ch == b' ' {
                            errors += 1;
                        }
                    } else {
                        i += 1;
                    }
                }
            } else {
                prev = Some(line);
            }
        }
        errors
    }

    #[test]
    fn bar_marker_aligns_to_bar_head_columns() {
        // 4 小節・各小節頭に 4 分音符。省略記法で `fm c r:4 r r r c ...` のように
        // なる。-b 1 (既定) なら 2,3,4 小節目の先頭トークン桁にマーカーが乗る。
        let s = score_with_one_note_per_bar(4, 480);
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        let body = melodic_clip_body(&dsl);
        let comments = comment_lines(&body);
        assert_eq!(comments.len(), 1, "コメント行は 1 本のはず: {body:?}");
        let comment = comments[0];
        // 本体行 (演奏行) を取り出す。
        let play_line = body
            .lines()
            .find(|l| l.trim_start().starts_with("fm"))
            .unwrap();
        // 各小節番号 (2,3,4) がコメント行に含まれる。
        assert!(comment.contains('2'), "2 がない: {comment:?}");
        assert!(comment.contains('3'), "3 がない: {comment:?}");
        assert!(comment.contains('4'), "4 がない: {comment:?}");
        // 1 (先頭小節) は出さない。コメント先頭の `// ` を除いた残りに 1 は無いはず。
        // 桁揃え検証: コメント行で `N` が現れる桁に、本体行で小節 N 先頭トークンが
        // 始まること。小節 2 の先頭トークンを本体から探し、その桁とコメントの
        // `2` の桁が一致する。
        let col_2_in_comment = comment.find('2').unwrap();
        // 本体行で小節 2 先頭 (3 つ目の音 = 2 拍の休符 r を挟んだ後) の桁を求める
        // のは複雑なので、ここでは「コメントの数字の桁にスペース以外の文字が
        // 本体行に存在する」ことだけ確認する (厳密な桁一致は round-trip と
        // 別テストで担保)。
        assert!(
            play_line.len() > col_2_in_comment,
            "本体行がコメントの桁より短い: play={play_line:?} comment={comment:?}"
        );
        assert_ne!(
            play_line.as_bytes()[col_2_in_comment],
            b' ',
            "小節 2 のマーカー桁が本体行で空白 (桁ズレ): play={play_line:?} comment={comment:?}"
        );
    }

    #[test]
    fn bars_per_marker_skips_intermediate_bars() {
        // -b 2: 2 小節ごと → 小節 3, 5, 7 ... にマーカー。先頭 1 と中間は出さない。
        // 6 小節で検証 (マーカー対象 = 3, 5)。
        let s = score_with_one_note_per_bar(6, 480);
        let opts = GenOptions {
            bars_per_marker: 2,
            ..Default::default()
        };
        let dsl = emit(&s, &opts).unwrap();
        let body = melodic_clip_body(&dsl);
        let comments = comment_lines(&body);
        let joined = comments.join(" ");
        assert!(joined.contains('3'), "3 がない: {joined:?}");
        assert!(joined.contains('5'), "5 がない: {joined:?}");
        // 2, 4, 6 はマーカー対象外。
        assert!(!joined.contains('2'), "2 が出てはいけない: {joined:?}");
        assert!(!joined.contains('4'), "4 が出てはいけない: {joined:?}");
        assert!(!joined.contains('6'), "6 が出てはいけない: {joined:?}");
    }

    #[test]
    fn bars_per_marker_zero_disables_comments() {
        // -b 0: コメント行を一切出さない。
        let s = score_with_one_note_per_bar(4, 480);
        let opts = GenOptions {
            bars_per_marker: 0,
            ..Default::default()
        };
        let dsl = emit(&s, &opts).unwrap();
        let body = melodic_clip_body(&dsl);
        assert!(
            comment_lines(&body).is_empty(),
            "コメント行が出ている: {body:?}"
        );
    }

    #[test]
    fn bar_marker_does_not_break_roundtrip() {
        // コメント行を含む生成 DSL が再び eval/compile できる (パーサがコメントを
        // 無視する) ことを確認する。
        let s = score_with_one_note_per_bar(5, 480);
        let got = compile_emitted_note_ons(&s);
        let want = expected_note_ons(&s);
        assert_eq!(got, want, "コメント行付き emit が round-trip しない");
    }

    /// 拍子 1/4 (1 小節 = 1 拍 = ppq tick) で、各小節に短い 1 音を置いた score を
    /// 作る。小節先頭トークンが出力上で隣接し、コメント行の数字が密に並ぶため
    /// パディングの off-by-one を検出しやすい。
    /// Builds a score in 1/4 time so bar-head tokens sit adjacent in the output.
    fn dense_one_token_per_bar(bars: u32) -> Score {
        let ppq = 480u32;
        let bar_t = ppq as u64; // 1/4 → 1 小節 = 1 拍
        let events = (0..bars as u64)
            .map(|b| Event::Note {
                start_tick: b * bar_t,
                end_tick: (b + 1) * bar_t,
                midi_note: 60,
                velocity: 100,
            })
            .collect();
        Score {
            ppq,
            initial_bpm: 120.0,
            time_signature: TimeSignature {
                numerator: 1,
                denominator: 4,
            },
            title: None,
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events,
            }],
        }
    }

    #[test]
    fn bar_markers_align_when_numbers_are_adjacent() {
        // 小節先頭トークンが隣接する密なケース。コメント行の小節番号が隣り合うと
        // パディングの off-by-one で桁ズレが起きやすい (回帰防止)。
        // 8 小節 = 番号 2..8 が密に並ぶ。
        let s = dense_one_token_per_bar(8);
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        assert!(
            !comment_lines(&melodic_clip_body(&dsl)).is_empty(),
            "コメント行が出ていない:\n{dsl}"
        );
        assert_eq!(
            count_column_misalignments(&dsl),
            0,
            "隣接する小節番号で桁ズレが発生:\n{dsl}"
        );
    }

    #[test]
    fn bar_markers_inside_loop_block_align() {
        // ループブロック内の小節先頭にもマーカーが付き、(...)*N の桁オフセット
        // 計算が正しいことを検証する。1/4 拍子で 4 小節を 2 回ループ → (...)*2。
        // 小節先頭が密に並ぶため、ループ内桁オフセットの off-by-one を検出できる。
        let ppq = 480u32;
        let bar_t = ppq as u64; // 1/4 → 1 小節 = 1 拍
        let inner: Vec<Event> = (0..4u64)
            .map(|b| Event::Note {
                start_tick: b * bar_t,
                end_tick: (b + 1) * bar_t,
                midi_note: 60,
                velocity: 100,
            })
            .collect();
        let s = Score {
            ppq,
            initial_bpm: 120.0,
            time_signature: TimeSignature {
                numerator: 1,
                denominator: 4,
            },
            title: None,
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events: vec![Event::LoopBlock {
                    start_tick: 0,
                    events: inner,
                    count: 2,
                }],
            }],
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        // ループ表記が出ていること。
        assert!(dsl.contains(")*2"), "ループ表記がない:\n{dsl}");
        // ループ内マーカー (1 周目の小節番号 2,3,4) が桁揃えされていること。
        assert_eq!(
            count_column_misalignments(&dsl),
            0,
            "ループ内マーカーで桁ズレが発生:\n{dsl}"
        );
    }

    #[test]
    fn comment_line_handles_adjacent_two_digit_markers() {
        // off-by-one ピンポイント: 2 桁番号の直後に次のマーカー桁が来る境界
        // (col == 既存長) で、padding が空白 0 個でちょうど揃うこと。
        // body 上で `AA`(col5) の直後 `BB`(col7) にマーカー 10, 11 を置く。
        let body = "  fm AABB\n";
        let markers = [
            BarMarker {
                abs_col: 5,
                bar_no: 10,
            },
            BarMarker {
                abs_col: 7,
                bar_no: 11,
            },
        ];
        let out = interleave_bar_comments(body, &markers, 1);
        // 期待: 本体行の直下にコメント行。`10` が col5、`11` が col7。
        //   "  fm AABB"
        //   "//   1011"   (// + 空白3 で col5=`1`, 続けて col7=`1`)
        let comment = out
            .lines()
            .find(|l| l.trim_start().starts_with("//"))
            .expect("コメント行がない");
        assert_eq!(comment, "//   1011", "桁揃えが崩れている: {comment:?}");
        // 念のため col5 に `1`(10 の頭), col7 に `1`(11 の頭) があること。
        assert_eq!(comment.as_bytes()[5], b'1');
        assert_eq!(comment.as_bytes()[7], b'1');
    }

    #[test]
    fn emit_includes_required_blocks() {
        let dsl = emit(&one_note_score(), &GenOptions::default()).unwrap();
        assert!(dsl.contains("device gen_device"));
        assert!(dsl.contains("instrument fm"));
        assert!(dsl.contains("clip fm_clip"));
        assert!(dsl.contains("tempo 120"));
        assert!(dsl.contains("scene gen_scene"));
        assert!(dsl.contains("play gen_scene"));
    }

    #[test]
    fn melodic_clip_has_single_note() {
        let dsl = emit(&one_note_score(), &GenOptions::default()).unwrap();
        // C4 / 4 分音符。初期キャリー (oct=4, dur=4) と一致するため省略形 `c` になる。
        // The single C4 quarter note matches the initial carry state (oct=4,
        // dur=4), so it is emitted in the shortest form `c`.
        let body = melodic_clip_body(&dsl);
        assert_eq!(body, "fm c", "省略形 `c` で書かれるべき: {body:?}");
    }

    #[test]
    fn rest_inserted_between_notes() {
        let mut s = one_note_score();
        s.tracks[0].events = vec![
            Event::Note {
                start_tick: 0,
                end_tick: 240, // 8 分
                midi_note: 60,
                velocity: 100,
            },
            Event::Note {
                start_tick: 480, // 4 分目から
                end_tick: 720,   // + 8 分
                midi_note: 62,
                velocity: 100,
            },
        ];
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        // 0-240 で c:4:8、240-480 が休符 8 分、480 から d:4:8。省略後は:
        //   c::8  (oct=4 は初期値と一致し省略、dur=8 は初期 4 と異なり明示)
        //   r     (休符 dur=8 は直前 c の 8 と一致し省略)
        //   d     (oct=4 dur=8 ともに直前と一致し両省略)
        // After shorthand: `c::8 r d`.
        let body = melodic_clip_body(&dsl);
        assert_eq!(body, "fm c::8 r d", "省略形が想定と異なる: {body:?}");
    }

    #[test]
    fn chord_emitted_with_bracket_notation() {
        let mut s = one_note_score();
        // 同 tick で C4 + E4 + G4 を 2 分鳴らす
        s.tracks[0].events = vec![
            Event::Note {
                start_tick: 0,
                end_tick: 960,
                midi_note: 60,
                velocity: 100,
            },
            Event::Note {
                start_tick: 0,
                end_tick: 960,
                midi_note: 64,
                velocity: 100,
            },
            Event::Note {
                start_tick: 0,
                end_tick: 960,
                midi_note: 67,
                velocity: 100,
            },
        ];
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        // 初期キャリー oct=4 と全構成音 (c4/e4/g4) が一致するため、和音内の oct は
        // 省略され `[c e g]:2` になる (dur=2 は初期 4 と異なり明示)。
        // All chord tones match the base octave (4), so the in-chord octaves are
        // omitted: `[c e g]:2`.
        let body = melodic_clip_body(&dsl);
        assert_eq!(
            body, "fm [c e g]:2",
            "和音内 oct 省略形が想定と異なる: {body:?}"
        );
    }

    /// 和音内でオクターブが基準と異なる音は明示され、一致する音は省略される。
    /// 基準 (clip 先頭の oct=4) に対し c4=省略, e5=明示, g4=省略 となる。
    ///
    /// In-chord octaves: tones matching the base octave are omitted, others are
    /// spelled out (`[c e:5 g]`).
    #[test]
    fn chord_inner_octave_partially_omitted() {
        let mut s = one_note_score();
        // 和音 [c4 e5 g4] を 2 分で (e だけ 1 オクターブ上)
        s.tracks[0].events = vec![
            Event::Note {
                start_tick: 0,
                end_tick: 960,
                midi_note: 60,
                velocity: 100,
            }, // c4
            Event::Note {
                start_tick: 0,
                end_tick: 960,
                midi_note: 76,
                velocity: 100,
            }, // e5
            Event::Note {
                start_tick: 0,
                end_tick: 960,
                midi_note: 67,
                velocity: 100,
            }, // g4
        ];
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        let body = melodic_clip_body(&dsl);
        assert_eq!(
            body, "fm [c e:5 g]:2",
            "和音内で基準と異なる oct のみ明示されるべき: {body:?}"
        );
        // 意味的等価性も確認する。
        let expected = expected_note_ons(&s);
        let actual = compile_emitted_note_ons(&s);
        assert_eq!(
            actual, expected,
            "和音内省略後の NoteOn が一致しない\nDSL:\n{dsl}"
        );
    }

    #[test]
    fn loop_block_expands_to_repeat_notation() {
        // (c4:4)*2 を期待する
        let inner = vec![Event::Note {
            start_tick: 0,
            end_tick: 480,
            midi_note: 60,
            velocity: 100,
        }];
        let s = Score {
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events: vec![Event::LoopBlock {
                    start_tick: 0,
                    events: inner,
                    count: 2,
                }],
            }],
            ..one_note_score()
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        assert!(dsl.contains("*2"), "expected (...)*2 in output:\n{}", dsl);
        assert!(dsl.contains("c:4:4"));
    }

    #[test]
    fn drum_track_emits_tr808_step_sequencer() {
        // kick (note 36) を 4 つ打ち
        let s = Score {
            tracks: vec![Track {
                name: "drums".into(),
                midi_channel: 10,
                kind: TrackKind::Drum,
                events: vec![
                    Event::Note {
                        start_tick: 0,
                        end_tick: 120,
                        midi_note: 36,
                        velocity: 100,
                    },
                    Event::Note {
                        start_tick: 480,
                        end_tick: 600,
                        midi_note: 36,
                        velocity: 100,
                    },
                    Event::Note {
                        start_tick: 960,
                        end_tick: 1080,
                        midi_note: 36,
                        velocity: 100,
                    },
                    Event::Note {
                        start_tick: 1440,
                        end_tick: 1560,
                        midi_note: 36,
                        velocity: 100,
                    },
                ],
            }],
            ..one_note_score()
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        // kit ブロックは生成しない。clip drums で use tr808 のみ。
        assert!(
            !dsl.contains("kit gen_kit"),
            "kit block must not be emitted"
        );
        assert!(!dsl.contains("kit tr808"), "kit block must not be emitted");
        assert!(dsl.contains("clip drums"), "clip drums がない: {dsl}");
        assert!(dsl.contains("use tr808"));
        assert!(dsl.contains("resolution 16"));
        // kick 行に x が 4 ステップ間隔で 4 つ
        assert!(dsl.contains("kick x...x...x...x..."), "kick row: {dsl}");
    }

    #[test]
    fn note_name_conversion_for_middle_c() {
        let (name, oct) = midi_to_note_name(60);
        assert_eq!(name, "c");
        assert_eq!(oct, 4);
    }

    #[test]
    fn note_name_conversion_for_a4() {
        let (name, oct) = midi_to_note_name(69);
        assert_eq!(name, "a");
        assert_eq!(oct, 4);
    }

    /// melodic clip は 8 小節ごとに改行が入る。
    /// 16 小節ぶんの 4 分音符を流し込み、ちょうど 8 小節境界 (= 32 ノート目
    /// の手前) で `\n` が挿入されることを確認する。
    ///
    /// Melodic output wraps to a new line every 8 bars.
    #[test]
    fn melodic_wraps_every_8_bars() {
        // 16 小節 = 64 拍の 4 分音符
        let ppq: u32 = 480;
        let mut events = Vec::new();
        for i in 0..64u64 {
            events.push(Event::Note {
                start_tick: i * ppq as u64,
                end_tick: (i + 1) * ppq as u64,
                midi_note: 60,
                velocity: 100,
            });
        }
        let s = Score {
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events,
            }],
            ..one_note_score()
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();

        // melodic clip 本体 (fm の clip ブロック) を抜き出して、改行数を数える
        let clip_start = dsl.find("clip fm_clip").expect("clip 開始位置");
        let after_open = dsl[clip_start..].find('{').unwrap() + clip_start + 1;
        let after_close_rel = dsl[after_open..].find('}').unwrap();
        let body = &dsl[after_open..after_open + after_close_rel];

        // body 内の改行数 = (開きブレース直後の 1 個) + (8 小節境界の 1 個) + (本体末尾の 1 個)
        // 16 小節 / 8 = 2 ブロックに分かれるので、本体行は 2 行になる。
        let newline_count = body.matches('\n').count();
        assert!(
            newline_count >= 3,
            "8 小節境界で改行が入るべき: body=\n{}",
            body
        );
    }

    /// 8 小節ごとに改行された melodic clip も DSL パーサで読み戻せる。
    /// Wrapped melodic clip survives a parser round-trip.
    #[test]
    fn melodic_wrapped_clip_round_trips_through_parser() {
        let ppq: u32 = 480;
        let mut events = Vec::new();
        for i in 0..64u64 {
            events.push(Event::Note {
                start_tick: i * ppq as u64,
                end_tick: (i + 1) * ppq as u64,
                midi_note: 60,
                velocity: 100,
            });
        }
        let s = Score {
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events,
            }],
            ..one_note_score()
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        let clip_start = dsl.find("clip fm_clip").expect("melodic clip 開始");
        let clip_end = clip_start + dsl[clip_start..].find("\n}").unwrap() + 2;
        let clip_src = &dsl[clip_start..clip_end];

        let (rest, _clip) = crate::parser::clip::parse_clip(clip_src)
            .unwrap_or_else(|e| panic!("clip 再パース失敗: {:?}\n---\n{}", e, clip_src));
        assert_eq!(rest, "");
    }

    /// drum row を `\` 継続で折り返した出力が DSL パーサで読み戻せる。
    /// 16 小節ぶん (32 step ヒット) を emit し、結果の drum clip だけを
    /// parse_clip に通して同じ row 長になることを確認する。
    ///
    /// Round-trip: the wrapped drum row can be re-parsed back into a single
    /// row of the expected length.
    #[test]
    fn drum_wrapped_row_round_trips_through_parser() {
        let ppq: u32 = 480;
        let mut drum_events = Vec::new();
        for i in 0..64u64 {
            drum_events.push(Event::Note {
                start_tick: i * ppq as u64,
                end_tick: i * ppq as u64 + 120,
                midi_note: 36,
                velocity: 100,
            });
        }
        let s = Score {
            tracks: vec![Track {
                name: "drums".into(),
                midi_channel: 10,
                kind: TrackKind::Drum,
                events: drum_events,
            }],
            ..one_note_score()
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        let clip_start = dsl.find("clip drums ").expect("drum clip 開始");
        // 該当 clip ブロックの `}` までを抜き出す
        let clip_end = clip_start + dsl[clip_start..].find("\n}").unwrap() + 2;
        let clip_src = &dsl[clip_start..clip_end];

        let (rest, clip) = crate::parser::clip::parse_clip(clip_src)
            .unwrap_or_else(|e| panic!("clip 再パース失敗: {:?}\n---\n{}", e, clip_src));
        assert_eq!(rest, "");
        match clip.body {
            crate::ast::clip::ClipBody::Drum(body) => {
                assert_eq!(body.rows.len(), 1, "1 row として読み戻されるべき");
                // 16 小節 × 16 step = 256 step
                assert_eq!(body.rows[0].hits.len(), 256);
            }
            _ => panic!("expected drum body"),
        }
    }

    /// drum clip も 8 小節ごとに `\` 行末継続で改行する。
    /// Drum output wraps every 8 bars with backslash continuation.
    #[test]
    fn drum_wraps_every_8_bars_with_backslash() {
        // 16 小節分の 4 分目ヒット (= 16*4 = 64 個の bd) を打つ。
        let ppq: u32 = 480;
        let mut events = Vec::new();
        for i in 0..64u64 {
            events.push(Event::Note {
                start_tick: i * ppq as u64,
                end_tick: i * ppq as u64 + 120,
                midi_note: 36,
                velocity: 100,
            });
        }
        let s = Score {
            tracks: vec![Track {
                name: "drums".into(),
                midi_channel: 10,
                kind: TrackKind::Drum,
                events,
            }],
            ..one_note_score()
        };
        let dsl = emit(&s, &GenOptions::default()).unwrap();
        // drum clip 本体を抜き出して `\` が含まれることを確認する。
        // 8 小節 = 128 step ごとに分割されるので、16 小節なら少なくとも 1 回。
        let clip_start = dsl.find("clip drums ").expect("drum clip 開始位置");
        let after_open = dsl[clip_start..].find('{').unwrap() + clip_start + 1;
        let after_close_rel = dsl[after_open..].find("\n}").unwrap();
        let body = &dsl[after_open..after_open + after_close_rel];
        assert!(
            body.contains('\\'),
            "drum row 内に `\\` 継続マーカーが含まれるべき: body=\n{}",
            body
        );
    }

    // ---- 省略記法 emit の意味的等価性 (round-trip) テスト ----

    /// score のノートを LoopBlock 展開しつつ絶対 tick の `(start_tick, midi)` 列に
    /// 平坦化する (検証用の期待値生成)。
    /// Flattens score notes (expanding loops) into absolute (start_tick, midi).
    fn flatten_score_notes(events: &[Event], base: u64, out: &mut Vec<(u64, u8)>) {
        for e in events {
            match e {
                Event::Note {
                    start_tick,
                    midi_note,
                    ..
                } => out.push((base + *start_tick, *midi_note)),
                Event::LoopBlock {
                    start_tick,
                    events: inner,
                    count,
                } => {
                    // 1 ループの長さ = inner の最終 end_tick - start_tick
                    let one = super::track_last_tick(inner).saturating_sub(*start_tick);
                    for k in 0..*count as u64 {
                        flatten_score_notes(inner, base + k * one, out);
                    }
                }
            }
        }
    }

    /// emit した DSL を eval → compile し、NoteOn の `(tick, note)` 列を返す。
    /// Emits, parses, compiles, and returns the NoteOn (tick, note) sequence.
    fn compile_emitted_note_ons(score: &Score) -> Vec<(u64, u8)> {
        use crate::engine::compiler::compile_clip;
        use crate::engine::evaluator::Evaluator;
        use crate::midi::message::MidiMessage;

        use crate::ast::clip::ClipBody;

        let dsl = emit(score, &GenOptions::default()).unwrap();
        let mut ev = Evaluator::new(score.initial_bpm as f64);
        ev.eval_source(&dsl)
            .unwrap_or_else(|e| panic!("emit 結果が eval 失敗: {e:?}\n---\n{dsl}"));
        let clock = ev.clock_snapshot();
        let mut result = Vec::new();
        // 命名正規化で instrument 名 (= clip 名の基) が変わるため、入力 score の
        // 名前ではなく registry に登録された Pitched clip を全て走査する。
        for clip_name in ev.registry().clip_names() {
            let clip = ev.registry().get_clip(&clip_name).unwrap();
            if !matches!(clip.body, ClipBody::Pitched(_)) {
                continue;
            }
            let compiled = compile_clip(clip, &clock, ev.registry()).unwrap();
            for e in &compiled.events {
                if let MidiMessage::NoteOn { note, .. } = e.message {
                    result.push((e.tick, note));
                }
            }
        }
        result.sort();
        result
    }

    /// score のノートを `(tick, midi)` 期待値に変換する。
    fn expected_note_ons(score: &Score) -> Vec<(u64, u8)> {
        let mut out = Vec::new();
        for t in &score.tracks {
            if t.kind != TrackKind::Melodic {
                continue;
            }
            flatten_score_notes(&t.events, 0, &mut out);
        }
        out.sort();
        out
    }

    /// 省略 emit した DSL を compile した NoteOn が、元 score のノートと一致する。
    /// オクターブ変化・休符・和音・付点・ループ・タイを含む複合ケースで検証する。
    ///
    /// Shorthand emission round-trips: compiling the emitted DSL yields the same
    /// NoteOn (tick, note) set as the source score across octaves, rests,
    /// chords, dotted durations, loops, and ties.
    #[test]
    fn shorthand_emission_round_trips_to_same_notes() {
        let ppq: u32 = 480;
        let q = ppq as u64; // 4 分音符
        let e8 = q / 2; // 8 分
        let s16 = q / 4; // 16 分
                         // 複合的なメロディ:
                         //   c4(4分) c4(4分) c5(8分) [休符 8分] e4(付点4分=720) g4(16分)
                         //   その後 (c4(8分) d4(8分))*2 のループ
        let mut events = vec![
            Event::Note {
                start_tick: 0,
                end_tick: q,
                midi_note: 60,
                velocity: 100,
            },
            Event::Note {
                start_tick: q,
                end_tick: 2 * q,
                midi_note: 60,
                velocity: 100,
            },
            Event::Note {
                start_tick: 2 * q,
                end_tick: 2 * q + e8,
                midi_note: 72,
                velocity: 100,
            },
            // 2*q+e8 .. 3*q は休符 (8分相当)
            Event::Note {
                start_tick: 3 * q,
                end_tick: 3 * q + q + e8,
                midi_note: 64,
                velocity: 100,
            }, // 付点4分
            Event::Note {
                start_tick: 3 * q + q + e8,
                end_tick: 3 * q + q + e8 + s16,
                midi_note: 67,
                velocity: 100,
            },
        ];
        // 和音 [c4 e4 g4] を 2 分で
        let chord_start = 6 * q;
        for m in [60u8, 64, 67] {
            events.push(Event::Note {
                start_tick: chord_start,
                end_tick: chord_start + 2 * q,
                midi_note: m,
                velocity: 100,
            });
        }
        // ループ (c4 8分, d4 8分)*2 を 8*q から
        let loop_start = 8 * q;
        events.push(Event::LoopBlock {
            start_tick: loop_start,
            events: vec![
                Event::Note {
                    start_tick: loop_start,
                    end_tick: loop_start + e8,
                    midi_note: 60,
                    velocity: 100,
                },
                Event::Note {
                    start_tick: loop_start + e8,
                    end_tick: loop_start + 2 * e8,
                    midi_note: 62,
                    velocity: 100,
                },
            ],
            count: 2,
        });

        let score = Score {
            ppq,
            initial_bpm: 120.0,
            time_signature: TimeSignature::default(),
            title: Some("roundtrip".into()),
            tracks: vec![Track {
                name: "lead".into(),
                midi_channel: 1,
                kind: TrackKind::Melodic,
                events,
            }],
        };

        let expected = expected_note_ons(&score);
        let actual = compile_emitted_note_ons(&score);
        assert_eq!(
            actual,
            expected,
            "省略 emit → compile した NoteOn が score と一致しない\nDSL:\n{}",
            emit(&score, &GenOptions::default()).unwrap()
        );
    }
}
