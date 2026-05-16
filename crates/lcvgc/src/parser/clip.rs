use nom::{bytes::complete::tag, character::complete::char, combinator::opt, IResult};

use crate::ast::clip::*;
use crate::parser::cell_normalize::{expand_bar_jump_cells, expand_pipe_cells, CellToken};
use crate::parser::clip_arpeggio::parse_arpeggio;
use crate::parser::clip_articulation::parse_articulation;
use crate::parser::clip_bar_jump::parse_bar_jump;
use crate::parser::clip_cc::{parse_cc_step, parse_cc_target, parse_cc_time};
use crate::parser::clip_drum::{
    expand_repetition, tokenize_drum_pattern, tokenize_probability_pattern,
};
use crate::parser::clip_note::parse_note_event;
use crate::parser::clip_options::parse_clip_options;
use crate::parser::clip_repetition::parse_repetition;
use crate::parser::common::{identifier, parse_u16, ws, ws1};

/// Parse a `clip NAME [options] { body }` block.
pub fn parse_clip(input: &str) -> IResult<&str, ClipDef> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("clip")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, options) = parse_clip_options(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = ws(input)?;

    // ドラムクリップかどうかを先読みで判定（"use"キーワードで始まるか）
    // Peek to determine if this is a drum clip (starts with "use" keyword)
    if input.trim_start().starts_with("use ") {
        let (input, body) = parse_drum_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('}')(input)?;
        Ok((
            input,
            ClipDef {
                name: name.to_string(),
                options,
                body: ClipBody::Drum(body),
            },
        ))
    } else {
        let (input, body) = parse_pitched_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('}')(input)?;
        Ok((
            input,
            ClipDef {
                name: name.to_string(),
                options,
                body: ClipBody::Pitched(body),
            },
        ))
    }
}

/// Parse the body of a pitched clip.
fn parse_pitched_body(mut input: &str) -> IResult<&str, PitchedClipBody> {
    let mut lines: Vec<PitchedLine> = Vec::new();
    let mut cc_automations = Vec::new();
    // 直前に `---` セパレータがあったかどうか。
    // `true` のとき、次に作成される PitchedLine は強制的に新レイヤー
    // (`is_layer_start = true`) として扱う。
    //
    // Whether the previous token was a `---` divider. When `true`, the next
    // line forced into a new layer regardless of instrument continuity.
    let mut force_layer_start = false;

    loop {
        let (rest, _) = ws(input)?;
        input = rest;

        // 閉じ波括弧の確認
        // Check for closing brace
        if input.starts_with('}') {
            break;
        }

        // `---` レイヤー分離行 (ハイフン3文字、前後空白可、独立行) を消費する。
        // 次に作成される PitchedLine は新レイヤー扱いになる。
        //
        // Consume a `---` divider line. The next PitchedLine becomes a fresh layer.
        if let Some(rest) = consume_dash_divider(input) {
            force_layer_start = true;
            input = rest;
            continue;
        }

        // CCオートメーションを試行（instrument.paramパターン）
        // Try CC automation (instrument.param pattern)
        if let Ok((_, _target)) = parse_cc_target(input) {
            // CC行 - まずタイム形式、次にステップ形式を試行
            // （タイム形式は value@bar.beat を要求するためステップ形式に誤マッチしない。
            //  ステップ形式は parse_u8 で値だけ部分マッチしてしまうため先に試行すると
            //  タイム形式の入力を誤消費する）
            // It's a CC line - try time first, then step
            // (Time format requires value@bar.beat so it won't false-match step format.
            //  Step format can partially match just the value via parse_u8,
            //  incorrectly consuming time-format input if tried first)
            if let Ok((rest, cc)) = parse_cc_time(input) {
                cc_automations.push(cc);
                input = rest;
                continue;
            }
            if let Ok((rest, cc)) = parse_cc_step(input) {
                cc_automations.push(cc);
                input = rest;
                continue;
            }
        }

        // 楽器名をパース
        // Parse instrument name
        let (rest, inst_name) = identifier(input)?;
        let (rest, _) = ws1(rest)?;

        // 要素をパース
        // Parse elements
        let mut elements = Vec::new();
        let mut current = rest;

        loop {
            let (r, _) = ws(current)?;
            current = r;

            if current.starts_with('}') || current.is_empty() {
                break;
            }

            // 同一または別の楽器、 `resolution` キーワード、 CC 行による改行を確認
            // (この行の終端)。
            // 別の楽器行 (例: `lead c:5:4` の次行 `bass c:3:4`) の判定は、
            // 次の identifier がそれ単独では note としてパースできない (= note 表記
            // でない) ことで行う。 単音 `c d eb` などは parse_note_event で識別子
            // として消費される側で、 ここでは「楽器名らしき長めの識別子」を
            // 検出して新行に切り替える。
            //
            // Detect end-of-line by peeking the next identifier.
            //   - same instrument name → break (existing behavior)
            //   - `resolution` keyword → break (existing behavior)
            //   - identifier followed by `.` → CC line, break (existing behavior)
            //   - identifier that does NOT parse as a note token → another
            //     instrument line, break (new behavior)
            if let Ok((_, next_ident)) = identifier(current) {
                if next_ident == inst_name || next_ident == "resolution" {
                    break;
                }
                let after_ident = &current[next_ident.len()..];
                if after_ident.starts_with('.') {
                    break;
                }
                // 識別子が note としてパース可能 (`c:3:8`, `eb`, `cm7:4:1` 等) なら
                // この line 内の要素として続行。 そうでなければ別楽器の新行とみなす。
                // 注意: `parse_note_event` は `bass` のような楽器名でも先頭の `b`
                // のみを音名として消費して成功してしまう。 そのため、
                // パース後の残り文字が「ノート要素として完結」したことも確認する
                // 必要がある。 楽器名と note の境界は、 note の直後が空白 / 改行 /
                // `}` / EOF / アーティキュレーション (`'` `g`) / アルペジオ (`(`) /
                // 付点 (`.`) のいずれかであることで判別する。
                //
                // The identifier may be a partial note prefix of an instrument
                // name (e.g. `bass` parses as note `b` with leftover `ass`).
                // Treat the token as a real note only when the parser leaves
                // a clean separator after it.
                let note_token_ok = match parse_note_event(current) {
                    Ok((after, _)) => {
                        let next_ch = after.chars().next();
                        matches!(
                            next_ch,
                            None | Some(' ' | '\t' | '\r' | '\n' | '}' | '\'' | '(' | '.')
                        ) || after.starts_with("g")
                            && after.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
                    }
                    Err(_) => false,
                };
                if !note_token_ok {
                    break;
                }
            }

            // `|` 拍境界スナップを試行
            // Try `|` beat-boundary snap. 単独トークン (=次が空白/改行/EOF/}) のみ受理。
            // Only accept a standalone `|` token (followed by whitespace, newline,
            // EOF, or closing brace) so future `||` (or similar) syntax stays open.
            if current.starts_with('|') {
                let after = &current[1..];
                let next_ch = after.chars().next();
                if matches!(next_ch, None | Some(' ' | '\t' | '\r' | '\n' | '}')) {
                    elements.push(PitchedElement::PipeSnap);
                    current = after;
                    continue;
                }
            }

            // 小節ジャンプを試行
            // Try bar jump
            if let Ok((r, bj)) = parse_bar_jump(current) {
                elements.push(PitchedElement::BarJump(bj));
                current = r;
                continue;
            }

            // リピートを試行
            // Try repetition
            if let Ok((r, rep)) = parse_repetition(current) {
                elements.push(PitchedElement::Repetition(rep));
                current = r;
                continue;
            }

            // コード括弧 [notes]:dur を試行
            // Try chord bracket [notes]:dur
            if current.starts_with('[') {
                let (r, chord) = parse_chord_bracket(current)?;
                elements.push(chord);
                current = r;
                continue;
            }

            // ノートイベントを試行（単音またはコード名）
            // Try note event (single note or chord name)
            if let Ok((r, note)) = parse_note_event(current) {
                let (r, art) = parse_articulation(r)?;
                // コード名に対するアルペジオを確認し、ChordName の arpeggio フィールドに格納する。
                // 単音 (`Single`) や休符 (`Rest`) には arp は付かない（パーサーは消費するだけ）。
                //
                // Detect a trailing `arp(...)` and attach it to ChordName. Single
                // notes and rests cannot carry an arpeggio; the parser just
                // consumes the syntax for them (existing behavior).
                let (r, _) = ws(r)?;
                if let Some((r2, arp)) = parse_arpeggio(r) {
                    let note_with_arp = match note {
                        crate::ast::clip_note::NoteEvent::ChordName {
                            root,
                            suffix,
                            octave,
                            duration,
                            dotted,
                            arpeggio: _,
                        } => crate::ast::clip_note::NoteEvent::ChordName {
                            root,
                            suffix,
                            octave,
                            duration,
                            dotted,
                            arpeggio: Some(arp),
                        },
                        other => other,
                    };
                    current = r2;
                    elements.push(PitchedElement::Note(note_with_arp, art));
                    continue;
                }
                elements.push(PitchedElement::Note(note, art));
                current = r;
                continue;
            }

            // 他にパースできるものがないため終了
            // Can't parse anything else, break
            break;
        }

        if !elements.is_empty() {
            // is_layer_start を判定する:
            //   - 直前に `---` があれば強制 true
            //   - 最初のラインなら true
            //   - 直前のラインと instrument が異なれば true
            //   - それ以外 (同 instrument の連続) は false → 連結
            //
            // Determine is_layer_start:
            //   - forced true if a `---` divider preceded this line
            //   - true for the first line in the body
            //   - true when the instrument differs from the previous line
            //   - otherwise false (consecutive same-instrument line → merged)
            let is_layer_start = force_layer_start
                || lines
                    .last()
                    .map(|prev| prev.instrument != inst_name)
                    .unwrap_or(true);
            lines.push(PitchedLine {
                instrument: inst_name.to_string(),
                elements,
                is_layer_start,
            });
            force_layer_start = false;
        }

        input = current;
    }

    Ok((
        input,
        PitchedClipBody {
            lines,
            cc_automations,
        },
    ))
}

/// `---` レイヤー分離行を消費する。
///
/// 入力先頭がちょうど 3 文字のハイフン (`---`) で始まり、その後ろが
/// 空白のみで改行 / EOF / `}` に到達する場合のみマッチし、消費後の残り入力を返す。
/// マッチしなければ `None`。
///
/// 仕様:
///   - ハイフンは厳密に 3 文字 (`----` のような 4 文字以上はマッチしない)
///   - 前後の空白 (タブ含む) は許容
///   - 行末は `\n` `\r\n` `\r` または EOF / `}` (clip body 終端)
///
/// Consume a `---` layer divider line. Matches only when the input starts
/// with exactly three hyphens, optionally surrounded by horizontal whitespace,
/// and terminated by a newline / EOF / closing brace. Returns the remaining
/// input if matched, or `None` otherwise.
pub(crate) fn consume_dash_divider(input: &str) -> Option<&str> {
    // 行頭の水平方向空白をスキップ
    // Skip leading horizontal whitespace
    let trimmed = input.trim_start_matches([' ', '\t']);
    let after_dashes = trimmed.strip_prefix("---")?;
    // 直後に追加のハイフンが続く場合 (`----` 等) はセパレータとして扱わない
    // Reject 4+ hyphens
    if after_dashes.starts_with('-') {
        return None;
    }
    // ハイフン後の水平方向空白をスキップ
    // Skip trailing horizontal whitespace
    let after_ws = after_dashes.trim_start_matches([' ', '\t']);
    // 行末確認: 改行 / EOF / `}` (clip body の終端)
    // Verify line end
    if let Some(rest) = after_ws.strip_prefix("\r\n") {
        Some(rest)
    } else if let Some(rest) = after_ws.strip_prefix('\n') {
        Some(rest)
    } else if let Some(rest) = after_ws.strip_prefix('\r') {
        Some(rest)
    } else if after_ws.is_empty() || after_ws.starts_with('}') {
        Some(after_ws)
    } else {
        None
    }
}

/// Parse a chord bracket: `[note1 note2 ...]:dur`
fn parse_chord_bracket(input: &str) -> IResult<&str, PitchedElement> {
    let (input, _) = char('[')(input)?;
    let mut notes = Vec::new();
    let mut current = input;

    loop {
        let (r, _) = ws(current)?;
        current = r;
        if current.starts_with(']') {
            current = &current[1..];
            break;
        }
        // 音名とオプションのオクターブをパース
        // Parse note_name and optional octave
        let (r, name) = crate::parser::common::note_name(current)?;
        let (r, oct) = opt(|i| {
            let (i, _) = char(':')(i)?;
            crate::parser::common::parse_u8(i)
        })(r)?;
        notes.push((name, oct));
        current = r;
    }

    // :duration をパース
    // Parse :duration
    let (current, dur) = if current.starts_with(':') {
        let (r, _) = char(':')(current)?;
        let (r, d) = parse_u16(r)?;
        (r, Some(d))
    } else {
        (current, None)
    };

    let (current, dotted) = opt(tag("."))(current)?;
    let (current, art) = parse_articulation(current)?;

    // アルペジオを確認
    // Check for arpeggio
    let (current, _) = ws(current)?;
    let (current, arp) = if let Some((r, a)) = parse_arpeggio(current) {
        (r, Some(a))
    } else {
        (current, None)
    };

    Ok((
        current,
        PitchedElement::ChordBracket {
            notes,
            duration: dur,
            dotted: dotted.is_some(),
            articulation: art,
            arpeggio: arp,
        },
    ))
}

/// Drum row の論理 1 行を読み出す。
///
/// 行末が `\` (バックスラッシュ) + 改行のとき、次行を同 row の続きとして
/// 連結する。`\` の後は行末まで空白のみを許容する。継続行先頭の空白は
/// trim され、前の論理行の末尾とは半角スペース 1 個でつなぐ。
///
/// Reads a single *logical* drum-row line, joining backslash-continued
/// physical lines. The trailing `\` plus optional whitespace and the newline
/// are stripped; leading whitespace on the continuation is trimmed; the
/// joined fragment is appended with a single space separator so the
/// downstream tokenizer treats them as a single pattern string.
///
/// # Returns
/// `(logical_line, rest)`:
/// - `logical_line` ... 連結後のパターン文字列 (改行を含まない)
/// - `rest`         ... 末尾の `\n` を消費した残りの input
fn read_logical_drum_line(input: &str) -> (String, &str) {
    let mut acc = String::new();
    let mut cursor = input;
    loop {
        let line_end = cursor.find('\n').unwrap_or(cursor.len());
        let line = &cursor[..line_end];

        // 行末の `\` 継続マーカーを検出する。`\` の後ろは空白のみを許容。
        // Detect a `\` continuation marker at end-of-line (trailing whitespace ok).
        let trimmed_end = line.trim_end();
        let has_continuation = trimmed_end.ends_with('\\');

        if has_continuation {
            // `\` を除いた本文を追加
            let body = &trimmed_end[..trimmed_end.len() - 1];
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(body.trim_start());

            if line_end == cursor.len() {
                // EOF: `\` で終わったが次行がない → そのまま終了
                cursor = &cursor[line_end..];
                break;
            }
            // 改行を消費して次の物理行へ
            cursor = &cursor[line_end + 1..];
            continue;
        }

        if !acc.is_empty() {
            acc.push(' ');
            acc.push_str(line.trim());
        } else {
            acc.push_str(line);
        }
        // 末尾の改行も消費 (なければ EOF)
        cursor = if line_end == cursor.len() {
            &cursor[line_end..]
        } else {
            &cursor[line_end + 1..]
        };
        break;
    }
    (acc, cursor)
}

/// Parse the body of a drum clip.
fn parse_drum_body(input: &str) -> IResult<&str, DrumClipBody> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("use")(input)?;
    let (input, _) = ws1(input)?;
    let (input, kit) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("resolution")(input)?;
    let (input, _) = ws1(input)?;
    let (input, resolution) = parse_u16(input)?;

    // 4/4拍子用
    // for 4/4 time
    let beats_per_step = resolution as usize / 4;

    let mut rows: Vec<crate::ast::clip_drum::DrumRow> = Vec::new();
    let mut cc_automations = Vec::new();
    let mut current = input;

    loop {
        let (r, _) = ws(current)?;
        current = r;

        if current.starts_with('}') || current.is_empty() {
            break;
        }

        // `---` レイヤー分離行を消費する (drum body では現状 no-op、将来拡張用に予約)。
        // Consume `---` divider (no-op in drum body; reserved for future layer support).
        if let Some(rest) = consume_dash_divider(current) {
            current = rest;
            continue;
        }

        // CCオートメーションを試行（タイム形式を先に試行）
        // Try CC automation (try time format first)
        if let Ok((_, _target)) = parse_cc_target(current) {
            if let Ok((r, cc)) = parse_cc_time(current) {
                cc_automations.push(cc);
                current = r;
                continue;
            }
            if let Ok((r, cc)) = parse_cc_step(current) {
                cc_automations.push(cc);
                current = r;
                continue;
            }
        }

        // 楽器名のパースを試行。失敗した場合は確率行の可能性がある
        // Try to parse instrument name. If it fails, this may be a probability row
        if let Ok((r, inst_name)) = identifier(current) {
            let (r, _) = ws(r)?;

            // 行末までパターンを読み取る (行末 `\` で次行に継続)
            // Read the pattern until end of line (backslash-continued lines join)
            let (pattern_owned, after_line) = read_logical_drum_line(r);
            let pattern = pattern_owned.trim();

            if pattern.is_empty() {
                current = after_line;
                continue;
            }

            // 確率行かどうかを確認（すべての文字が0-9、`.`、`|`、`>`、`(`、`)`、`*`、スペース）
            // Check if this could be a probability row (all chars are 0-9, `.`, `|`, `>`,
            // `(`, `)`, `*`, space)
            let is_prob = pattern.chars().all(|c| {
                c.is_ascii_digit() || matches!(c, '.' | '|' | '>' | '(' | ')' | '*' | ' ')
            });

            if is_prob && !rows.is_empty() {
                // 直前のドラム行に対する確率行 — `(...)*N` 展開 → トークナイズ →
                // `|` 解決 → `>N` 解決 → flat な確率値列
                // Probability row for the previous drum row.
                let prob = build_probability_row(pattern, beats_per_step, resolution as usize)
                    .map_err(|_| {
                        nom::Err::Failure(nom::error::Error::new(
                            current,
                            nom::error::ErrorKind::Char,
                        ))
                    })?;
                if let Some(last) = rows.last_mut() {
                    last.probability = Some(prob);
                }
            } else {
                // ヒットパターン行 — `(...)*N` 展開 → トークナイズ → `|` 解決 →
                // `>N` 解決 → flat な HitSymbol 列
                // Hit pattern row.
                let hits = build_drum_hits(pattern, beats_per_step, resolution as usize).map_err(
                    |_| {
                        nom::Err::Failure(nom::error::Error::new(
                            current,
                            nom::error::ErrorKind::Char,
                        ))
                    },
                )?;
                rows.push(crate::ast::clip_drum::DrumRow {
                    instrument: inst_name.to_string(),
                    hits,
                    probability: None,
                });
            }

            current = after_line;
        } else {
            // 楽器名がない行 — 確率行としてパース
            // Line without instrument name — parse as probability row
            let (pattern_owned, after_line) = read_logical_drum_line(current);
            let pattern = pattern_owned.trim();

            if pattern.is_empty() {
                current = after_line;
                continue;
            }

            // 確率行かどうかを確認（すべての文字が0-9、`.`、`|`、`>`、`(`、`)`、`*`、スペース）
            // Check if all chars are probability-compatible.
            let is_prob = pattern.chars().all(|c| {
                c.is_ascii_digit() || matches!(c, '.' | '|' | '>' | '(' | ')' | '*' | ' ')
            });

            if is_prob && !rows.is_empty() {
                // パイプ・小節ジャンプ含めて新パイプラインで解決
                // Resolve via the new cell-token pipeline (incl. `|` / `>N`).
                let prob = build_probability_row(pattern, beats_per_step, resolution as usize)
                    .map_err(|_| {
                        nom::Err::Failure(nom::error::Error::new(
                            current,
                            nom::error::ErrorKind::Char,
                        ))
                    })?;
                if let Some(last) = rows.last_mut() {
                    last.probability = Some(prob);
                }
            } else {
                return Err(nom::Err::Failure(nom::error::Error::new(
                    current,
                    nom::error::ErrorKind::Char,
                )));
            }

            current = after_line;
        }
    }

    Ok((
        current,
        DrumClipBody {
            kit: kit.to_string(),
            resolution,
            rows,
            cc_automations,
        },
    ))
}

/// ドラム行 (string) を新パイプラインで `Vec<HitSymbol>` まで解決する。
///
/// パイプライン:
///   1. `expand_repetition` で `(...)*N` を string 段で展開する。
///   2. `tokenize_drum_pattern` で `CellToken<HitSymbol>` 列に変換する。
///   3. `expand_pipe_cells` で `|` を拍境界スナップに変換する (skip = `Rest`)。
///   4. `expand_bar_jump_cells` で `>N` を絶対位置スナップに変換する。
///      `steps_per_bar` は `resolution` を 1 小節分のセル数として使う。
///      長さの padding/truncate は呼び出し側 (= シーケンサ) に任せるため、
///      `total_steps = None` を指定する。
///
/// Resolve a drum row string into a flat `Vec<HitSymbol>` via the new
/// cell-token pipeline.
///
/// # Arguments
/// * `pattern` - ドラム行の生文字列 / raw drum row text
/// * `beats_per_step` - 1 拍あたりのセル数 / cells per beat
/// * `steps_per_bar` - 1 小節あたりのセル数 (= resolution) / cells per bar
///
/// # Returns
/// `Ok(Vec<HitSymbol>)` — 展開済みのヒット列 / expanded hit sequence
///
/// # Errors
/// 未知のシンボル文字、`>` の後ろの数字欠落、`BarJump(0)` などでエラーを返す。
/// Returns an error on unknown symbols, `>` without digits, or `BarJump(0)`.
fn build_drum_hits(
    pattern: &str,
    beats_per_step: usize,
    steps_per_bar: usize,
) -> Result<Vec<crate::ast::clip_drum::HitSymbol>, String> {
    let after_rep = expand_repetition(pattern);
    let tokens = tokenize_drum_pattern(&after_rep)?;
    let piped = expand_pipe_cells(
        &tokens,
        beats_per_step,
        &crate::ast::clip_drum::HitSymbol::Rest,
    );
    let flat = expand_bar_jump_cells(
        &piped,
        steps_per_bar,
        None,
        &crate::ast::clip_drum::HitSymbol::Rest,
    )?;
    Ok(flat)
}

/// 確率行 (string) を新パイプラインで `Vec<u8>` まで解決する。
///
/// `tokenize_probability_pattern` を使う以外は `build_drum_hits` と同じ流れ。
/// `expand_pipe_cells` / `expand_bar_jump_cells` の `skip_cell` には
/// 確率行の意味で「常に発音」を表す 100 を渡す (= drum 行 `Rest` に相当)。
///
/// Resolve a probability row string into a flat `Vec<u8>` via the new
/// cell-token pipeline.
///
/// # Arguments
/// * `pattern` - 確率行の生文字列 / raw probability row text
/// * `beats_per_step` - 1 拍あたりのセル数 / cells per beat
/// * `steps_per_bar` - 1 小節あたりのセル数 (= resolution) / cells per bar
///
/// # Returns
/// `Ok(Vec<u8>)` — 展開済みの確率値列 / expanded probability sequence
///
/// # Errors
/// 未知のシンボル、`>N` 不正でエラーを返す。
/// Returns an error on unknown symbols or invalid `>N`.
fn build_probability_row(
    pattern: &str,
    beats_per_step: usize,
    steps_per_bar: usize,
) -> Result<Vec<u8>, String> {
    let after_rep = expand_repetition(pattern);
    let tokens: Vec<CellToken<u8>> = tokenize_probability_pattern(&after_rep)?;
    // 確率行の `skip_cell` は「ステップなし = 常に発音 (100)」。
    // これは drum 行で `.` = Rest と扱うのと同じ「埋め役」。
    let piped = expand_pipe_cells(&tokens, beats_per_step, &100u8);
    let flat = expand_bar_jump_cells(&piped, steps_per_bar, None, &100u8)?;
    Ok(flat)
}

/// 繰り返し content 文字列をピッチド要素列にパースする。
/// Repetition の content（括弧の中身）を PitchedElement のリストに変換する。
///
/// Parse repetition content string into a vector of pitched elements.
/// Converts the content inside parentheses of a Repetition into PitchedElement list.
pub fn parse_repetition_content(content: &str) -> Result<Vec<PitchedElement>, String> {
    let mut elements = Vec::new();
    let mut current = content;

    while let Ok((r, _)) = ws(current) {
        current = r;

        if current.is_empty() {
            break;
        }

        // `|` 拍境界スナップを試行
        // Try `|` beat-boundary snap (standalone token only)
        if current.starts_with('|') {
            let after = &current[1..];
            let next_ch = after.chars().next();
            if matches!(next_ch, None | Some(' ' | '\t' | '\r' | '\n')) {
                elements.push(PitchedElement::PipeSnap);
                current = after;
                continue;
            }
        }

        // 小節ジャンプを試行
        // Try bar jump
        if let Ok((r, bj)) = parse_bar_jump(current) {
            elements.push(PitchedElement::BarJump(bj));
            current = r;
            continue;
        }

        // リピートを試行（ネスト対応）
        // Try repetition (supports nesting)
        if let Ok((r, rep)) = parse_repetition(current) {
            elements.push(PitchedElement::Repetition(rep));
            current = r;
            continue;
        }

        // コード括弧を試行
        // Try chord bracket
        if current.starts_with('[') {
            if let Ok((r, chord)) = parse_chord_bracket(current) {
                elements.push(chord);
                current = r;
                continue;
            }
        }

        // ノートイベントを試行（単音またはコード名）
        // Try note event (single note or chord name)
        if let Ok((r, note)) = parse_note_event(current) {
            let (r, art) = parse_articulation(r).map_err(|e| format!("{:?}", e))?;
            elements.push(PitchedElement::Note(note, art));
            current = r;
            continue;
        }

        break;
    }

    Ok(elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip_note::{ChordSuffix, NoteEvent};
    use crate::ast::common::NoteName;

    #[test]
    fn test_simple_pitched_clip() {
        let input = r#"clip bass_a [bars 1] {
  bass c:3:8 c eb f::4 g::2
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(clip.name, "bass_a");
        assert_eq!(clip.options.bars, Some(1));
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                assert_eq!(body.lines[0].instrument, "bass");
                assert_eq!(body.lines[0].elements.len(), 5);
            }
            _ => panic!("expected pitched"),
        }
    }

    #[test]
    fn test_simple_drum_clip() {
        let input = r#"clip drums_a [bars 1] {
  use tr808
  resolution 16

  bd    x...x...x...x...
  snare ....x.......x...
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(clip.name, "drums_a");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.kit, "tr808");
                assert_eq!(body.resolution, 16);
                assert_eq!(body.rows.len(), 2);
                assert_eq!(body.rows[0].instrument, "bd");
                assert_eq!(body.rows[0].hits.len(), 16);
                assert_eq!(body.rows[1].instrument, "snare");
            }
            _ => panic!("expected drum"),
        }
    }

    #[test]
    fn test_clip_no_options() {
        let input = r#"clip bass_poly {
  bass c:3:4 eb::4 f::4
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(clip.name, "bass_poly");
        assert_eq!(clip.options.bars, None);
    }

    #[test]
    fn test_pitched_chord_name() {
        let input = r#"clip chords [bars 4] {
  keys cm7:4:2
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                assert_eq!(body.lines[0].elements.len(), 1);
                match &body.lines[0].elements[0] {
                    PitchedElement::Note(NoteEvent::ChordName { root, suffix, .. }, _) => {
                        assert_eq!(*root, NoteName::C);
                        assert_eq!(*suffix, ChordSuffix::Min7);
                    }
                    other => panic!("expected chord name, got {:?}", other),
                }
            }
            _ => panic!("expected pitched"),
        }
    }

    /// `cm arp(random, 4)` のような ChordName + arp が、ChordName.arpeggio に
    /// 正しく取り込まれることを検証する。
    /// `cm arp(random, 4)` should be parsed with arpeggio attached to ChordName.
    #[test]
    fn test_pitched_chord_name_with_arpeggio() {
        let input = r#"clip arp_clip [bars 1] {
  bass cm arp(random, 4)
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines[0].elements.len(), 1);
                match &body.lines[0].elements[0] {
                    PitchedElement::Note(
                        NoteEvent::ChordName {
                            root,
                            suffix,
                            arpeggio,
                            ..
                        },
                        _,
                    ) => {
                        assert_eq!(*root, NoteName::C);
                        assert_eq!(*suffix, ChordSuffix::Min);
                        let arp = arpeggio.expect("arpeggio should be Some");
                        assert_eq!(
                            arp.direction,
                            crate::parser::clip_arpeggio::ArpeggioDirection::Random
                        );
                        assert_eq!(arp.resolution, Some(4));
                    }
                    other => panic!("expected chord name, got {:?}", other),
                }
            }
            _ => panic!("expected pitched"),
        }
    }

    /// 第2引数省略 `cm arp(up)` でも ChordName.arpeggio に取り込まれること。
    /// `cm arp(up)` (no resolution) is also captured into ChordName.
    #[test]
    fn test_pitched_chord_name_with_arpeggio_no_resolution() {
        let input = r#"clip arp_clip [bars 1] {
  bass cm:4:8 arp(up)
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => match &body.lines[0].elements[0] {
                PitchedElement::Note(NoteEvent::ChordName { arpeggio, .. }, _) => {
                    let arp = arpeggio.expect("arpeggio should be Some");
                    assert_eq!(
                        arp.direction,
                        crate::parser::clip_arpeggio::ArpeggioDirection::Up
                    );
                    assert_eq!(arp.resolution, None);
                }
                other => panic!("expected chord name, got {:?}", other),
            },
            _ => panic!("expected pitched"),
        }
    }

    /// CCタイム形式を含むピッチドクリップのパーステスト
    /// Test parsing a pitched clip containing CC time-format automation
    #[test]
    fn test_pitched_clip_with_cc_time() {
        let input = r#"clip bass_a [bars 4] {
  bass c:3:4 eb::4
  vbass.cutoff 40@1.1-100@4.4
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                assert_eq!(body.cc_automations.len(), 1);
                match &body.cc_automations[0] {
                    crate::ast::clip_cc::CcAutomation::Time(time) => {
                        assert_eq!(time.target.instrument, "vbass");
                        assert_eq!(time.target.param, "cutoff");
                        assert_eq!(time.segments.len(), 1);
                        assert_eq!(time.segments[0].from.value, 40);
                        assert_eq!(time.segments[0].from.bar, 1);
                        assert_eq!(time.segments[0].from.beat, 1);
                    }
                    other => panic!("expected Time CC, got {:?}", other),
                }
            }
            _ => panic!("expected pitched"),
        }
    }

    /// CCステップ形式が引き続きパースできることのテスト
    /// Test that CC step-format automation still parses correctly
    #[test]
    fn test_pitched_clip_with_cc_step() {
        let input = r#"clip bass_b [bars 1] {
  bass c:3:4
  vbass.cutoff 0 10 20 30
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                assert_eq!(body.cc_automations.len(), 1);
                match &body.cc_automations[0] {
                    crate::ast::clip_cc::CcAutomation::Step(step) => {
                        assert_eq!(step.target.instrument, "vbass");
                        assert_eq!(step.target.param, "cutoff");
                        let expected: Vec<crate::parser::cell_normalize::CellToken<Option<u8>>> =
                            [0u8, 10, 20, 30]
                                .iter()
                                .map(|v| crate::parser::cell_normalize::CellToken::Cell(Some(*v)))
                                .collect();
                        assert_eq!(step.cells, expected);
                    }
                    other => panic!("expected Step CC, got {:?}", other),
                }
            }
            _ => panic!("expected pitched"),
        }
    }

    /// ドラムクリップでのCCタイム形式テスト
    /// Test CC time-format automation in drum clip
    #[test]
    fn test_drum_clip_with_cc_time() {
        let input = r#"clip drums_a [bars 1] {
  use tr808
  resolution 16
  bd    x...x...x...x...
  vdrum.cutoff 40@1.1-100@1.4
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.rows.len(), 1);
                assert_eq!(body.cc_automations.len(), 1);
                match &body.cc_automations[0] {
                    crate::ast::clip_cc::CcAutomation::Time(time) => {
                        assert_eq!(time.target.instrument, "vdrum");
                        assert_eq!(time.target.param, "cutoff");
                    }
                    other => panic!("expected Time CC, got {:?}", other),
                }
            }
            _ => panic!("expected drum"),
        }
    }

    /// `|` 拍境界スナップが PitchedElement::PipeSnap としてパースされる
    /// `|` parses as PitchedElement::PipeSnap.
    #[test]
    fn test_pitched_pipe_snap() {
        let input = r#"clip bass_pipe [bars 1] {
  bass c:3:8 c | c c
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                let elems = &body.lines[0].elements;
                // c c | c c → 5 要素
                assert_eq!(elems.len(), 5);
                assert!(matches!(elems[2], PitchedElement::PipeSnap));
            }
            _ => panic!("expected pitched"),
        }
    }

    /// 末尾の `|` も認識される
    /// Trailing `|` is recognized.
    #[test]
    fn test_pitched_trailing_pipe_snap() {
        let input = r#"clip bass_pipe [bars 1] {
  bass c:3:8 c |
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                let elems = &body.lines[0].elements;
                assert_eq!(elems.len(), 3);
                assert!(matches!(elems[2], PitchedElement::PipeSnap));
            }
            _ => panic!("expected pitched"),
        }
    }

    #[test]
    fn test_multiline_pitched() {
        let input = r#"clip bass_a [bars 2] {
  bass c:3:8 c eb f::4 g::2
  bass ab:3:8 g f eb::4 c::2
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 2);
                assert_eq!(body.lines[0].instrument, "bass");
                assert_eq!(body.lines[1].instrument, "bass");
            }
            _ => panic!("expected pitched"),
        }
    }

    /// drum 行に新仕様 `|` (拍境界スナップ) を入れた場合の挙動を検証する。
    /// 行の途中で `|` を入れて 1 拍だけ書き、その先を `>3` で 3 小節目頭に
    /// ジャンプして 2 音だけ書く。 resolution = 16 (1 小節 16 セル) なので
    /// 全体は最終的に 3 小節目末尾までで 48 セル相当 (= bar 3 末尾 = step 47)
    /// が埋まる…のではなく、 `expand_bar_jump_cells` の `total_steps = None`
    /// は `steps_per_bar` (= 16) の倍数に切り上げるため、最終長は 48 セル。
    ///
    /// Verify drum row supports new `|` (beat-boundary snap) and `>N`
    /// (bar-absolute jump).
    #[test]
    fn test_drum_row_with_pipe_and_bar_jump() {
        let input = r#"clip drums_b [bars 4] {
  use tr808
  resolution 16
  bd    x.| >3 xx
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.rows.len(), 1);
                let hits = &body.rows[0].hits;
                // 期待:
                //   step 0..2  = [Normal, Rest]   (x.)
                //   step 2..4  = [Rest, Rest]      (| で 4 セル境界まで埋め)
                //   step 4..32 = 全 Rest          (bar 2 = `.` 埋め)
                //   step 32..34 = [Normal, Normal] (>3 で bar 3 頭、xx)
                //   step 34..48 = 全 Rest          (bar 3 末尾までの埋め)
                // total_steps = None なので 48 (= 3 bar 分) で揃う。
                assert_eq!(hits.len(), 48, "3 bar (48 セル) に切り上げられるはず");
                use crate::ast::clip_drum::HitSymbol::*;
                assert_eq!(hits[0], Normal);
                assert_eq!(hits[1], Rest);
                assert_eq!(hits[2], Rest);
                assert_eq!(hits[3], Rest);
                // bar 1 末尾と bar 2 は全 Rest。
                for (i, h) in hits.iter().enumerate().take(32).skip(4) {
                    assert_eq!(*h, Rest, "bar1 末尾 / bar2 は Rest のはず (idx={i})");
                }
                assert_eq!(hits[32], Normal);
                assert_eq!(hits[33], Normal);
                for (i, h) in hits.iter().enumerate().take(48).skip(34) {
                    assert_eq!(*h, Rest, "bar3 末尾は Rest のはず (idx={i})");
                }
            }
            _ => panic!("expected drum"),
        }
    }

    /// `|` 超過時の切り落とし: 1 拍 = 4 セルなのに `x x x x x |` と 5 セル書いて
    /// `|` で 1 拍境界に強制スナップすると、末尾 1 セルが切り落とされる。
    /// Truncation on overrun: 5 cells + `|` → 4 cells.
    #[test]
    fn test_drum_row_pipe_truncates_overrun() {
        let input = r#"clip drums_c [bars 1] {
  use tr808
  resolution 16
  bd    xxxxx|
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Drum(body) => {
                let hits = &body.rows[0].hits;
                // 5 セル → 4 セルに切り落とし。`total_steps = None` で
                // steps_per_bar (= 16) の倍数に切り上げ → 最終 16 セル。
                assert_eq!(hits.len(), 16);
                use crate::ast::clip_drum::HitSymbol::*;
                assert_eq!(hits[0], Normal);
                assert_eq!(hits[1], Normal);
                assert_eq!(hits[2], Normal);
                assert_eq!(hits[3], Normal);
                // 5 セル目はカット。残りは Rest 埋め。
                for (i, h) in hits.iter().enumerate().take(16).skip(4) {
                    assert_eq!(*h, Rest, "idx={i} は Rest のはず");
                }
            }
            _ => panic!("expected drum"),
        }
    }

    /// drum row の行末 `\` で次行へ継続できる。
    /// 8 小節分のステップを 2 行に分けて書いても、論理的には 1 row として
    /// パースされる。
    ///
    /// Backslash-continuation lets a drum row span multiple physical lines.
    #[test]
    fn test_drum_row_backslash_continuation() {
        // bars=2 / resolution 16 → 32 ステップを 16+16 で 2 行に分割。
        let input = r#"clip drums_x [bars 2] {
  use tr808
  resolution 16
  ch  xxxxxxxxxxxxxxxx \
      xxxxxxxxxxxxxxxx
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.rows.len(), 1, "1 row として認識されるべき");
                let row = &body.rows[0];
                assert_eq!(row.instrument, "ch");
                assert_eq!(row.hits.len(), 32, "32 ステップ連結されるべき");
                use crate::ast::clip_drum::HitSymbol::Normal;
                for (i, h) in row.hits.iter().enumerate() {
                    assert_eq!(*h, Normal, "idx={i} は Normal のはず");
                }
            }
            _ => panic!("expected drum"),
        }
    }

    /// drum row の継続は 3 行以上にも対応する。
    /// More than two physical lines also concatenate correctly.
    #[test]
    fn test_drum_row_backslash_continuation_three_lines() {
        let input = r#"clip drums_x [bars 3] {
  use tr808
  resolution 16
  ch  xxxxxxxxxxxxxxxx \
      xxxxxxxxxxxxxxxx \
      xxxxxxxxxxxxxxxx
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.rows.len(), 1);
                assert_eq!(body.rows[0].hits.len(), 48);
            }
            _ => panic!("expected drum"),
        }
    }

    /// `\` の後に余分な空白があっても継続する。
    /// Trailing whitespace after `\` is permitted.
    #[test]
    fn test_drum_row_backslash_with_trailing_space() {
        let input = "clip d [bars 2] {\n  use tr808\n  resolution 16\n  ch  xxxxxxxxxxxxxxxx \\   \n      xxxxxxxxxxxxxxxx\n}";
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.rows[0].hits.len(), 32);
            }
            _ => panic!("expected drum"),
        }
    }
}
