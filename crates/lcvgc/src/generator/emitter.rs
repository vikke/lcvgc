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
    emit_clips(score, &mut out)?;
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
fn emit_clips(score: &Score, out: &mut String) -> Result<(), GeneratorError> {
    for t in &score.tracks {
        let bars = bars_for_track(t, score);
        let clip_name = clip_name_for(t);
        match t.kind {
            TrackKind::Melodic => {
                emit_melodic_clip(&clip_name, t, score, bars, out)?;
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
) -> Result<(), GeneratorError> {
    out.push_str(&format!("clip {} [bars {}] {{\n", clip_name, bars));
    let mut cursor: u64 = 0;
    let mut body = String::new();
    body.push_str(&format!("  {}", track.name));
    // clip 単位でキャリー状態を初期化する (parser のデフォルトに合わせる)。
    let mut carry = EmitCarry::new();
    emit_event_sequence(
        &track.events,
        score,
        &mut cursor,
        &mut body,
        &track.name,
        Some(BARS_PER_WRAP),
        &mut carry,
    )?;
    body.push('\n');
    out.push_str(&body);
    out.push_str("}\n\n");
    Ok(())
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
/// Serializes a sequence of melodic events into the clip body. When
/// `wrap_every_n_bars` is `Some(n)`, inserts a newline + indent each time the
/// bar cursor crosses an n-bar boundary. Pass `None` for recursive
/// LoopBlock expansion so the `(...)*N` payload stays on a single line.
fn emit_event_sequence(
    events: &[Event],
    score: &Score,
    cursor: &mut u64,
    out: &mut String,
    _track_name: &str,
    wrap_every_n_bars: Option<u32>,
    carry: &mut EmitCarry,
) -> Result<(), GeneratorError> {
    // 1) ノートと LoopBlock を時系列で取り出す
    let mut items: Vec<&Event> = events.iter().collect();
    items.sort_by_key(|e| e.start_tick());

    let bar_t = bar_ticks(score);

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
                emit_event_sequence(
                    inner,
                    score,
                    &mut inner_cursor,
                    &mut inner_out,
                    _track_name,
                    None,
                    &mut inner_carry,
                )?;
                // ループ末尾のキャリー状態を親に反映する。
                *carry = inner_carry;
                let one_iter_ticks = inner_cursor.saturating_sub(*start_tick);
                out.push_str(" (");
                out.push_str(inner_out.trim_start());
                out.push_str(&format!(" )*{}", count));
                *cursor += one_iter_ticks * (*count as u64);
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
