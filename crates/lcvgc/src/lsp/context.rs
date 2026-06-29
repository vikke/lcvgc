use super::completion::{CompletionItem, CompletionKind, CompletionProvider};
use crate::ast::scale::ScaleType;
use crate::domain::pitch::NoteName;
use crate::engine::registry::Registry;
use crate::midi::port::list_ports;
use crate::parser::scale::parse_scale;

/// カーソル位置のコンテキスト
#[derive(Debug, PartialEq)]
pub enum CompletionContext {
    /// トップレベル（ブロック外）: ブロックキーワードを提案
    TopLevel,
    /// ブロックキーワードの後（名前入力中）: 補完なし
    AfterBlockKeyword,
    /// device ブロック内の行頭
    DeviceBody,
    /// device 内 "port " の後: 実 MIDI ポート名を提案（lcvgc プロセスから取得）
    /// Inside device block after "port ": suggest actual MIDI port names (fetched from lcvgc process)
    DeviceAfterPort,
    /// instrument ブロック内の行頭
    InstrumentBody,
    /// instrument 内 "device " の後: デバイス名を提案
    InstrumentAfterDevice,
    /// instrument 内 "note " の後: ノート名を提案
    InstrumentAfterNote,
    /// instrument 内 数値期待位置: 補完なし
    NumberExpected,
    /// kit ブロック内の行頭
    KitBody,
    /// kit 内 "device " の後: デバイス名を提案
    KitAfterDevice,
    /// clip ブロック内の **行頭** (pitched): 楽器名のみを提案する。
    ///
    /// pitched clip の各行は `INSTRUMENT_NAME <element>*` という構造で、
    /// 行頭は instrument 名を書く位置である。音名やコード名は instrument 名の
    /// **あと** にしか書けないため、行頭では出さない。
    ///
    /// At the start of a pitched-clip line, only instrument names are valid.
    /// Note names and chord names appear strictly after the instrument token.
    PitchedClipLineStart {
        /// clip ローカルの `[scale ...]` で確定した (ルート音, スケール種)。
        /// 行頭では使わないが、後続の `PitchedClipAfterInstrument` 判定と
        /// 構造を揃えるため保持する。
        scale: Option<(NoteName, ScaleType)>,
    },
    /// clip ブロック内の **instrument 名直後** (pitched): 音名・コード名を提案する。
    ///
    /// 行頭の instrument 名 (`chord`, `lead`, `bass` 等) を書き終え、空白を
    /// 1 個以上越えた位置。`scale` の解決状態に応じて以下を返す:
    /// - scale が解決できているとき: スケール構成音 7 音 + ダイアトニックコード 7 つ。
    ///   半音階 17 音フォールバックは出さない。
    /// - `None` のとき: 半音階 17 音を提示する。
    ///
    /// **instrument 名は含めない** (本位置では文法的に書けない)。
    ///
    /// Right after an instrument token on a pitched clip line. Instrument
    /// names are excluded here because they cannot legally appear in this
    /// position.
    PitchedClipAfterInstrument {
        /// clip ローカルの `[scale ...]` で確定した (ルート音, スケール種)
        scale: Option<(NoteName, ScaleType)>,
    },
    /// clip ブロック内の行頭（drum）: use/resolution + kit楽器名
    DrumClipBody,
    /// clip 内 "use " の後: キット名を提案
    ClipAfterUse,
    /// scene ブロック内の行頭: clip名 + tempo
    SceneBody,
    /// session ブロック内の行頭: scene名
    SessionBody,
    /// session 内 "[" の後: repeat/loop
    SessionAfterBracket,
    /// "tempo " の後（トップレベル）: 補完なし
    AfterTempo,
    /// "scale " の後: ノート名（ルート音）
    AfterScale,
    /// "scale <note> " の後: スケールタイプ
    AfterScaleNote,
    /// "play " の後: scene名 + session キーワード
    AfterPlay,
    /// "play session " の後: session名
    AfterPlaySession,
    /// "stop " の後: clip名
    AfterStop,
    /// "include " の後: 補完なし
    AfterInclude,
    /// "var " の後: 補完なし
    AfterVar,
    /// clip オプション "[" 内: bars/time/scale（使用済みオプションを除外）
    /// Clip option inside `[`: bars/time/scale (excluding already used options)
    ClipOption { used_options: Vec<String> },
    /// clip オプション "[scale " 内: ノート名
    ClipOptionAfterScale,
    /// clip オプション "[scale <note> " 内: スケールタイプ
    ClipOptionAfterScaleNote,
    /// `arp(` 直後: アルペジオ方向 (up / down / updown / random) を提案
    /// After `arp(`: suggest arpeggio directions.
    AfterArpOpen,
    /// `arp(<direction>, ` 直後: 主要音価 (4 / 8 / 16 / 32) を提案
    /// After `arp(<direction>, `: suggest common note resolutions.
    AfterArpComma,
}

/// ソーステキスト内の指定オフセットまでの brace depth と
/// 最後の開きブレースの位置を算出する。
/// コメント（行コメント `//` とブロックコメント `/* */`）をスキップする。
pub fn brace_depth_at(source: &str, offset: usize) -> (i32, Option<usize>) {
    let bytes = source.as_bytes();
    let end = offset.min(bytes.len());
    let mut depth = 0i32;
    let mut last_open = None;
    let mut i = 0;
    while i < end {
        if i + 1 < end && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // 行コメント: 改行までスキップ
            while i < end && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < end && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // ブロックコメント: 閉じ */ までスキップ（ネスト対応）
            i += 2;
            let mut cdepth = 1u32;
            while i + 1 < end && cdepth > 0 {
                if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    cdepth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    cdepth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if i < end && bytes[i] == b'"' {
            // 文字列リテラル内のブレースはスキップ
            i += 1;
            while i < end && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1; // エスケープ
                }
                i += 1;
            }
            if i < end {
                i += 1; // 閉じ " をスキップ
            }
            continue;
        }
        match bytes[i] {
            b'{' => {
                depth += 1;
                last_open = Some(i);
            }
            b'}' => {
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    (depth, last_open)
}

/// 最後の開きブレース位置からブロックキーワードを特定する
pub fn find_enclosing_block_keyword(source: &str, brace_pos: usize) -> Option<&str> {
    let before = &source[..brace_pos];
    let trimmed = before.trim_end();
    // ブレースの前は "keyword name" or "keyword name [options...]"
    // まず ] をスキップ（clip options）
    let trimmed = if trimmed.ends_with(']') {
        let bracket_start = trimmed.rfind('[')?;
        trimmed[..bracket_start].trim_end()
    } else {
        trimmed
    };
    // "keyword name" の keyword 部分を抽出
    // 最後の行を取得
    let last_line = trimmed.lines().last()?.trim();
    // 最初の単語がキーワード
    let first_word = last_line.split_whitespace().next()?;
    match first_word {
        "device" | "instrument" | "kit" | "clip" | "scene" | "session" => Some(first_word),
        _ => None,
    }
}

/// カーソル位置の行テキスト（行頭からカーソルまで）を取得する
pub fn line_text_to_cursor(source: &str, offset: usize) -> &str {
    let start = source[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    &source[start..offset]
}

/// clip ブロック内に "use " があるかチェック（drum clip 判定）
pub fn clip_has_use(source: &str, brace_pos: usize, cursor_offset: usize) -> bool {
    let block_content = &source[brace_pos + 1..cursor_offset];
    block_content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("use ")
    })
}

/// カーソル位置の補完コンテキストを判定する
pub fn determine_completion_context(source: &str, offset: usize) -> CompletionContext {
    let (depth, last_open) = brace_depth_at(source, offset);
    let line = line_text_to_cursor(source, offset);
    let trimmed = line.trim_start();

    // トップレベル（ブレース外）
    if depth <= 0 {
        return determine_toplevel_context(trimmed);
    }

    // ブロック内
    let brace_pos = match last_open {
        Some(p) => p,
        None => return CompletionContext::TopLevel,
    };

    let block_kw = find_enclosing_block_keyword(source, brace_pos);

    match block_kw {
        Some("device") => determine_device_context(trimmed),
        Some("instrument") => determine_instrument_context(trimmed),
        Some("kit") => determine_kit_context(trimmed, depth),
        Some("clip") => determine_clip_context(trimmed, source, brace_pos, offset),
        Some("scene") => determine_scene_context(trimmed),
        Some("session") => determine_session_context(trimmed),
        _ => CompletionContext::TopLevel,
    }
}

/// トップレベルコンテキストを判定する
fn determine_toplevel_context(trimmed: &str) -> CompletionContext {
    if trimmed.is_empty() {
        return CompletionContext::TopLevel;
    }

    // "keyword " のパターンをチェック
    let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
    let keyword = parts[0];

    match keyword {
        "clip" => {
            if parts.len() >= 3 {
                let rest = parts[2];
                // rest 内の最後の "[" を探し、その後に "]" がなければオプション補完中
                if let Some(last_bracket) = rest.rfind('[') {
                    if !rest[last_bracket..].contains(']') {
                        let after_bracket = &rest[last_bracket + 1..];
                        let before_bracket = &rest[..last_bracket];
                        let used_options = extract_used_options(before_bracket);
                        return parse_clip_bracket_option(after_bracket, used_options);
                    }
                }
                CompletionContext::AfterBlockKeyword
            } else if parts.len() >= 2 {
                CompletionContext::AfterBlockKeyword
            } else {
                CompletionContext::TopLevel
            }
        }
        "device" | "instrument" | "kit" | "scene" | "session" => {
            if parts.len() >= 2 {
                CompletionContext::AfterBlockKeyword
            } else {
                CompletionContext::TopLevel
            }
        }
        "tempo" => {
            if parts.len() >= 2 {
                CompletionContext::AfterTempo
            } else {
                CompletionContext::TopLevel
            }
        }
        "scale" => {
            if parts.len() >= 3 {
                CompletionContext::AfterScaleNote
            } else if parts.len() >= 2 {
                CompletionContext::AfterScale
            } else {
                CompletionContext::TopLevel
            }
        }
        "play" => {
            if parts.len() >= 3 && parts[1] == "session" {
                CompletionContext::AfterPlaySession
            } else if parts.len() >= 2 {
                CompletionContext::AfterPlay
            } else {
                CompletionContext::TopLevel
            }
        }
        "stop" => {
            if parts.len() >= 2 {
                CompletionContext::AfterStop
            } else {
                CompletionContext::TopLevel
            }
        }
        "include" => {
            if parts.len() >= 2 {
                CompletionContext::AfterInclude
            } else {
                CompletionContext::TopLevel
            }
        }
        "var" => {
            if parts.len() >= 2 {
                CompletionContext::AfterVar
            } else {
                CompletionContext::TopLevel
            }
        }
        _ => CompletionContext::TopLevel,
    }
}

/// device ブロック内のコンテキストを判定する
///
/// 行頭からカーソル位置までのトリム済みテキスト `trimmed` を受け取り、
/// 「port キーワード + 半角空白」で始まる場合は `DeviceAfterPort`（実 MIDI ポート名補完）、
/// そうでない場合は `DeviceBody`（device ブロック内のキーワード補完）を返す。
///
/// # Arguments
/// * `trimmed` - カーソル位置の行頭から先頭空白を除去した文字列
///
/// # Returns
/// 判定された `CompletionContext`
fn determine_device_context(trimmed: &str) -> CompletionContext {
    // "port " (port + 半角空白1個以上) の後ろなら実ポート名補完
    // "port" 単体（後続空白なし）はまだ DeviceBody（port キーワード自体の補完を許容）
    if trimmed.starts_with("port ") {
        return CompletionContext::DeviceAfterPort;
    }
    CompletionContext::DeviceBody
}

/// instrument ブロック内のコンテキストを判定する
fn determine_instrument_context(trimmed: &str) -> CompletionContext {
    if trimmed.is_empty() {
        return CompletionContext::InstrumentBody;
    }
    if trimmed.starts_with("device ") {
        return CompletionContext::InstrumentAfterDevice;
    }
    if trimmed.starts_with("note ") {
        return CompletionContext::InstrumentAfterNote;
    }
    if trimmed.starts_with("channel ")
        || trimmed.starts_with("gate_normal ")
        || trimmed.starts_with("gate_staccato ")
        || trimmed.starts_with("velocity_normal ")
        || trimmed.starts_with("velocity_accent ")
        || trimmed.starts_with("velocity_ghost ")
    {
        return CompletionContext::NumberExpected;
    }
    if trimmed.starts_with("cc ") {
        // "cc alias_name cc_number" - エイリアスやCC番号は自由入力
        return CompletionContext::AfterBlockKeyword;
    }
    if trimmed.starts_with("var ") {
        return CompletionContext::AfterVar;
    }
    CompletionContext::InstrumentBody
}

/// kit ブロック内のコンテキストを判定する
fn determine_kit_context(trimmed: &str, depth: i32) -> CompletionContext {
    if trimmed.is_empty() {
        return CompletionContext::KitBody;
    }
    // depth > 1 の場合、kit 内の楽器定義ブロック ({ channel N, note X }) 内
    if depth > 1 {
        return CompletionContext::NumberExpected;
    }
    if trimmed.starts_with("device ") {
        return CompletionContext::KitAfterDevice;
    }
    CompletionContext::KitBody
}

/// 閉じたブラケット `[...] ` から使用済みオプションキーワードを抽出する
/// Extract used option keywords from closed brackets `[...]`
fn extract_used_options(text: &str) -> Vec<String> {
    let mut used = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        if let Some(close) = rest[open..].find(']') {
            let inner = rest[open + 1..open + close].trim();
            if let Some(keyword) = inner.split_whitespace().next() {
                used.push(keyword.to_string());
            }
            rest = &rest[open + close + 1..];
        } else {
            break;
        }
    }
    used
}

/// `[` の後の文字列から clip オプションコンテキストを判定する
/// Determine clip option context from the string after `[`
fn parse_clip_bracket_option(after_bracket: &str, used_options: Vec<String>) -> CompletionContext {
    let trimmed = after_bracket.trim_start();
    if trimmed.starts_with("scale ") {
        let after_scale = trimmed.strip_prefix("scale ").unwrap().trim_start();
        if after_scale.contains(' ') {
            return CompletionContext::ClipOptionAfterScaleNote;
        }
        return CompletionContext::ClipOptionAfterScale;
    }
    CompletionContext::ClipOption { used_options }
}

/// 行頭からカーソルまでのテキストを見て、`arp(` 直後 / `arp(<dir>, ` 直後を判定する。
///
/// - `arp(` の `(` 以降にカーソル位置で **方向トークンが完成していない** 場合 →
///   `AfterArpOpen`
/// - `arp(<direction>,` の `,` 以降に未確定の数値しか無い場合 → `AfterArpComma`
///
/// それ以外（既に方向や数値が完成している、`)` で閉じている等）は `None`。
///
/// Inspect the line text up to the cursor and decide whether we are right
/// after `arp(` (direction completion) or after `arp(<dir>,` (resolution
/// completion).
fn detect_arp_completion(line: &str) -> Option<CompletionContext> {
    // 直近の `arp(` を探す。複数 arp が同じ行にある場合は最後のものを採用。
    let arp_pos = line.rfind("arp(")?;
    let after_open = &line[arp_pos + 4..];
    // すでに `)` で閉じられている場合は補完対象外
    if after_open.contains(')') {
        return None;
    }
    // `,` で区切る（最大1個まで意味あり）
    if let Some(comma_pos) = after_open.find(',') {
        let after_comma = &after_open[comma_pos + 1..];
        // カンマ以降に空白以外で英字が現れていれば、ユーザーが何かを書き始めている。
        // 数値だけ書きかけのケース（例: "8"）も補完を出して問題ないため、
        // 「アルファベットを含む = 補完を返さない」程度の素朴な判定にする。
        if after_comma.chars().any(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        Some(CompletionContext::AfterArpComma)
    } else {
        // `(` の後にまだカンマが無い → 方向トークンの位置
        // ユーザーが既に方向を書き始めていても、補完候補は同じ 4 件で問題ない。
        Some(CompletionContext::AfterArpOpen)
    }
}

/// clip ブロック内のコンテキストを判定する
fn determine_clip_context(
    trimmed: &str,
    source: &str,
    brace_pos: usize,
    cursor_offset: usize,
) -> CompletionContext {
    // "[" 内のオプション判定
    let line_before = &source[..cursor_offset];
    let last_line_start = line_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let full_line = &source[last_line_start..cursor_offset];
    if let Some(bracket_pos) = full_line.rfind('[') {
        let in_bracket = &full_line[bracket_pos + 1..];
        // "[" の直後 or "[bars " 等の後
        if !full_line[bracket_pos..].contains(']') {
            let before_bracket = &full_line[..bracket_pos];
            let used_options = extract_used_options(before_bracket);
            return parse_clip_bracket_option(in_bracket, used_options);
        }
    }

    // `arp(` 直後 / `arp(<dir>, ` 直後の判定。clip body 内のどの位置でも有効。
    // Detect `arp(` (suggest direction) or `arp(<dir>, ` (suggest resolution).
    if let Some(arp_ctx) = detect_arp_completion(full_line) {
        return arp_ctx;
    }

    if trimmed.is_empty() {
        if clip_has_use(source, brace_pos, cursor_offset) {
            return CompletionContext::DrumClipBody;
        }
        let scale = extract_clip_local_scale(source, brace_pos);
        return CompletionContext::PitchedClipLineStart { scale };
    }
    if trimmed.starts_with("use ") {
        return CompletionContext::ClipAfterUse;
    }
    if trimmed.starts_with("resolution ") {
        return CompletionContext::NumberExpected;
    }
    if clip_has_use(source, brace_pos, cursor_offset) {
        CompletionContext::DrumClipBody
    } else {
        // pitched clip 行内: instrument 名を書き終えたか (= カーソルまでに
        // 「最初の非空白トークンの直後の空白」が存在するか) で位置を区別する。
        // 行頭から最初の非空白文字列を取り、その先に空白が現れたら
        // 「instrument 名直後」 (= 音名/コード期待) とみなす。
        //
        // Inside a pitched-clip line: differentiate "line start" vs "after the
        // instrument token" by checking whether the first non-whitespace token
        // is already followed by whitespace at the cursor position.
        let scale = extract_clip_local_scale(source, brace_pos);
        let line_text = full_line.trim_start();
        let after_instrument = line_text
            .find(char::is_whitespace)
            .is_some_and(|i| i < line_text.len());
        if after_instrument {
            CompletionContext::PitchedClipAfterInstrument { scale }
        } else {
            CompletionContext::PitchedClipLineStart { scale }
        }
    }
}

/// clip オープニング行から `[scale ROOT TYPE]` を抽出する
///
/// `clip name [bars 1] [scale c minor] {` の `{` 位置 `brace_pos` を受け取り、
/// その手前の `[ ... ]` ブロックを順に走査して `scale` オプションを探す。
/// 複数 `[scale ...]` が書かれた場合は **最後の指定** を優先する
/// (DSL 的にも後勝ち)。パースに失敗 / 未指定なら `None`。
///
/// # Arguments
/// * `source` - ソース全体
/// * `brace_pos` - clip ブロックの `{` の位置
///
/// # Returns
/// 解決した `(NoteName, ScaleType)` または `None`
fn extract_clip_local_scale(source: &str, brace_pos: usize) -> Option<(NoteName, ScaleType)> {
    let header = source[..brace_pos].trim_end();
    // ヘッダ末尾から `[ ... ]` を順に取り出し、`scale ROOT TYPE` を後勝ちで採用
    let mut found: Option<(NoteName, ScaleType)> = None;
    let mut rest = header;
    while let Some(open) = rest.find('[') {
        let after_open = &rest[open + 1..];
        let close_rel = after_open.find(']')?;
        let inner = after_open[..close_rel].trim();
        if let Some(args) = inner.strip_prefix("scale ") {
            // `parse_scale` は "scale ..." 全体を期待するので prefix を再構築
            let candidate = format!("scale {}", args.trim());
            if let Ok((_, sd)) = parse_scale(&candidate) {
                found = Some((sd.root, sd.scale_type));
            }
        }
        rest = &after_open[close_rel + 1..];
    }
    found
}

/// scene ブロック内のコンテキストを判定する
fn determine_scene_context(trimmed: &str) -> CompletionContext {
    if trimmed.is_empty() {
        return CompletionContext::SceneBody;
    }
    if trimmed.starts_with("tempo ") {
        return CompletionContext::AfterTempo;
    }
    CompletionContext::SceneBody
}

/// session ブロック内のコンテキストを判定する
fn determine_session_context(trimmed: &str) -> CompletionContext {
    if trimmed.is_empty() {
        return CompletionContext::SessionBody;
    }
    // "[" の中（repeat/loop）
    if let Some(bracket_pos) = trimmed.rfind('[') {
        if !trimmed[bracket_pos..].contains(']') {
            return CompletionContext::SessionAfterBracket;
        }
    }
    CompletionContext::SessionBody
}

/// コンテキストに基づいて補完候補を生成する
pub fn build_completion_items(ctx: &CompletionContext, registry: &Registry) -> Vec<CompletionItem> {
    match ctx {
        CompletionContext::TopLevel => CompletionProvider::keyword_completions(),

        CompletionContext::AfterBlockKeyword
        | CompletionContext::AfterTempo
        | CompletionContext::AfterVar
        | CompletionContext::NumberExpected => {
            vec![]
        }

        CompletionContext::AfterInclude => {
            // TODO: base_pathがbuild_completion_itemsに渡されていないため、
            // 現段階ではregistryから取得できない。将来的にbase_pathを渡す必要がある。
            // 今は空のベクターを返す（後方互換）
            // TODO: base_path is not passed to build_completion_items yet,
            // so it cannot be obtained from the registry. base_path needs to be passed in the future.
            // For now, return an empty vector (backward compatible)
            vec![]
        }

        CompletionContext::DeviceBody => CompletionProvider::device_body_completions(),

        CompletionContext::DeviceAfterPort => {
            // 実 MIDI 出力ポート一覧を補完候補として返す。`list_ports()` は環境
            // 依存の I/O を含むため、ALSA 等が見えない CI などでは Err になる
            // 場合がある。Err 時は空 vec を返してフォールバックする(補完が出ない
            // だけで他機能には影響しない)。
            //
            // Returns the system MIDI output ports as completion candidates. The
            // `list_ports()` call may fail in environments without ALSA/midir
            // (e.g. CI runners); we silently fall back to an empty list so other
            // completion paths remain unaffected.
            let ports = list_ports().unwrap_or_default();
            CompletionProvider::midi_port_completions(&ports)
        }

        CompletionContext::InstrumentBody => CompletionProvider::instrument_body_completions(),

        CompletionContext::InstrumentAfterDevice => {
            CompletionProvider::identifier_completions(&registry.device_names(), "device")
        }

        CompletionContext::InstrumentAfterNote => CompletionProvider::note_completions(),

        CompletionContext::KitBody => CompletionProvider::kit_body_completions(),

        CompletionContext::KitAfterDevice => {
            CompletionProvider::identifier_completions(&registry.device_names(), "device")
        }

        CompletionContext::PitchedClipLineStart { .. } => {
            // 行頭は instrument 名だけが書ける位置。音名/コードは出さない。
            // Only instrument names are valid here; note names and chord names
            // cannot appear at the start of a pitched-clip line.
            CompletionProvider::identifier_completions(&registry.instrument_names(), "instrument")
        }

        CompletionContext::PitchedClipAfterInstrument { scale } => {
            // instrument 名を書き終えた位置: 音名/コードのみを提示する。
            // instrument 名は本位置では文法的に書けないため候補に含めない。
            //
            // Scale resolution: clip-local `[scale ...]` > registry top-level
            // `scale` > unresolved. When resolved, return in-scale notes (7) +
            // diatonic chords (7); otherwise fall back to chromatic 17 notes.
            let resolved_scale: Option<(NoteName, ScaleType)> =
                scale.or_else(|| registry.scale().map(|s| (s.root, s.scale_type)));

            let mut items = Vec::new();
            if let Some((root, scale_type)) = resolved_scale {
                items.extend(CompletionProvider::scale_note_completions(root, scale_type));
                items.extend(CompletionProvider::diatonic_completions(root, scale_type));
            } else {
                items.extend(CompletionProvider::note_completions());
            }
            items
        }

        CompletionContext::DrumClipBody => {
            let mut items = CompletionProvider::drum_clip_body_completions();
            // kit の楽器名を候補に追加
            for kit in registry.kits().values() {
                for inst in &kit.instruments {
                    items.push(CompletionItem {
                        label: inst.name.clone(),
                        detail: Some(format!(
                            "kit instrument (ch{})",
                            inst.channel.as_one_based()
                        )),
                        kind: CompletionKind::Identifier,
                        sort_text: None,
                    });
                }
            }
            items
        }

        CompletionContext::ClipAfterUse => {
            CompletionProvider::identifier_completions(&registry.kit_names(), "kit")
        }

        CompletionContext::SceneBody => {
            let mut items =
                CompletionProvider::identifier_completions(&registry.clip_names(), "clip");
            items.extend(CompletionProvider::scene_body_keyword_completions());
            items
        }

        CompletionContext::SessionBody => {
            CompletionProvider::identifier_completions(&registry.scene_names(), "scene")
        }

        CompletionContext::SessionAfterBracket => {
            CompletionProvider::session_entry_option_completions()
        }

        CompletionContext::AfterScale => CompletionProvider::note_completions(),

        CompletionContext::AfterScaleNote => CompletionProvider::scale_type_completions(),

        CompletionContext::AfterPlay => {
            let mut items =
                CompletionProvider::identifier_completions(&registry.scene_names(), "scene");
            items.extend(CompletionProvider::play_keyword_completions());
            items
        }

        CompletionContext::AfterPlaySession => {
            CompletionProvider::identifier_completions(&registry.session_names(), "session")
        }

        CompletionContext::AfterStop => {
            CompletionProvider::identifier_completions(&registry.clip_names(), "clip")
        }

        CompletionContext::ClipOption { ref used_options } => {
            CompletionProvider::clip_option_completions()
                .into_iter()
                .filter(|item| !used_options.contains(&item.label))
                .collect()
        }

        CompletionContext::ClipOptionAfterScale => CompletionProvider::note_completions(),

        CompletionContext::ClipOptionAfterScaleNote => CompletionProvider::scale_type_completions(),

        CompletionContext::AfterArpOpen => CompletionProvider::arpeggio_direction_completions(),

        CompletionContext::AfterArpComma => CompletionProvider::arpeggio_resolution_completions(),
    }
}

/// カーソル位置周辺の識別子を抽出する
pub fn word_at_offset(source: &str, offset: usize) -> Option<String> {
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

/// バイトオフセットから (行番号, 列番号) のタプルへ変換する
/// tower-lsp 非依存のピュア実装
pub fn offset_to_line_col(source: &str, offset: usize) -> (u32, u32) {
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
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_to_line_col_start() {
        let (line, col) = offset_to_line_col("hello\nworld", 0);
        assert_eq!(line, 0);
        assert_eq!(col, 0);
    }

    #[test]
    fn offset_to_line_col_second_line() {
        let (line, col) = offset_to_line_col("hello\nworld", 6);
        assert_eq!(line, 1);
        assert_eq!(col, 0);
    }

    #[test]
    fn offset_to_line_col_middle_second_line() {
        let (line, col) = offset_to_line_col("hello\nworld", 9);
        assert_eq!(line, 1);
        assert_eq!(col, 3);
    }

    #[test]
    fn offset_to_line_col_end() {
        let (line, col) = offset_to_line_col("hello\nworld", 11);
        assert_eq!(line, 1);
        assert_eq!(col, 5);
    }

    #[test]
    fn offset_to_line_col_empty() {
        let (line, col) = offset_to_line_col("", 0);
        assert_eq!(line, 0);
        assert_eq!(col, 0);
    }

    // --- brace_depth_at tests ---

    #[test]
    fn brace_depth_no_braces() {
        let (depth, last) = brace_depth_at("tempo 120", 9);
        assert_eq!(depth, 0);
        assert!(last.is_none());
    }

    #[test]
    fn brace_depth_inside_block() {
        let src = "device synth {\n  port \"IAC\"\n}";
        let (depth, last) = brace_depth_at(src, 20); // inside block
        assert_eq!(depth, 1);
        assert_eq!(last, Some(13));
    }

    #[test]
    fn brace_depth_after_block() {
        let src = "device synth {\n  port \"IAC\"\n}";
        let (depth, _) = brace_depth_at(src, src.len());
        assert_eq!(depth, 0);
    }

    #[test]
    fn brace_depth_skips_line_comment() {
        let src = "// {\ndevice synth {\n}";
        let (depth, _) = brace_depth_at(src, src.len());
        assert_eq!(depth, 0);
    }

    #[test]
    fn brace_depth_skips_block_comment() {
        let src = "/* { */ device synth {\n}";
        let (depth, _) = brace_depth_at(src, src.len());
        assert_eq!(depth, 0);
    }

    #[test]
    fn brace_depth_skips_string() {
        let src = "device synth {\n  port \"{}\"\n}";
        let (depth, _) = brace_depth_at(src, src.len());
        assert_eq!(depth, 0);
    }

    // --- find_enclosing_block_keyword tests ---

    #[test]
    fn find_block_keyword_device() {
        let src = "device synth {";
        assert_eq!(find_enclosing_block_keyword(src, 13), Some("device"));
    }

    #[test]
    fn find_block_keyword_clip_with_options() {
        let src = "clip bass_a [bars 1] {";
        assert_eq!(find_enclosing_block_keyword(src, 21), Some("clip"));
    }

    #[test]
    fn find_block_keyword_scene() {
        let src = "scene intro {";
        assert_eq!(find_enclosing_block_keyword(src, 12), Some("scene"));
    }

    #[test]
    fn find_block_keyword_session() {
        let src = "session main {";
        assert_eq!(find_enclosing_block_keyword(src, 13), Some("session"));
    }

    // --- determine_completion_context tests ---

    #[test]
    fn ctx_toplevel_empty() {
        assert_eq!(
            determine_completion_context("", 0),
            CompletionContext::TopLevel
        );
    }

    #[test]
    fn ctx_toplevel_newline() {
        assert_eq!(
            determine_completion_context("tempo 120\n", 10),
            CompletionContext::TopLevel
        );
    }

    #[test]
    fn ctx_after_device_keyword() {
        let src = "device ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterBlockKeyword
        );
    }

    #[test]
    fn ctx_after_tempo_keyword() {
        let src = "tempo ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterTempo
        );
    }

    #[test]
    fn ctx_after_scale_keyword() {
        let src = "scale ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterScale
        );
    }

    #[test]
    fn ctx_after_scale_note() {
        let src = "scale c ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterScaleNote
        );
    }

    #[test]
    fn ctx_after_play() {
        let src = "play ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterPlay
        );
    }

    #[test]
    fn ctx_after_play_session() {
        let src = "play session ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterPlaySession
        );
    }

    #[test]
    fn ctx_after_stop() {
        let src = "stop ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterStop
        );
    }

    #[test]
    fn ctx_after_include() {
        let src = "include ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterInclude
        );
    }

    #[test]
    fn ctx_after_var() {
        let src = "var ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterVar
        );
    }

    #[test]
    fn ctx_device_body() {
        let src = "device synth {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::DeviceBody
        );
    }

    #[test]
    fn ctx_instrument_body() {
        let src = "instrument bass {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::InstrumentBody
        );
    }

    #[test]
    fn ctx_instrument_after_device() {
        let src = "instrument bass {\n  device ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::InstrumentAfterDevice
        );
    }

    #[test]
    fn ctx_instrument_after_note() {
        let src = "instrument bd {\n  note ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::InstrumentAfterNote
        );
    }

    #[test]
    fn ctx_instrument_after_channel() {
        let src = "instrument bass {\n  channel ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::NumberExpected
        );
    }

    #[test]
    fn ctx_instrument_after_gate_normal() {
        let src = "instrument bass {\n  gate_normal ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::NumberExpected
        );
    }

    #[test]
    fn ctx_kit_body() {
        let src = "kit tr808 {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::KitBody
        );
    }

    #[test]
    fn ctx_kit_after_device() {
        let src = "kit tr808 {\n  device ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::KitAfterDevice
        );
    }

    /// 行頭 (instrument 名期待位置) では PitchedClipLineStart を返す。
    /// bars だけの clip は scale 未確定 → None。
    #[test]
    fn ctx_pitched_clip_line_start() {
        let src = "clip bass_a [bars 1] {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipLineStart { scale: None }
        );
    }

    /// clip-local `[scale c major]` が PitchedClipLineStart に伝搬する
    #[test]
    fn ctx_pitched_clip_line_start_with_clip_local_scale_c_major() {
        let src = "clip x [scale c major] {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipLineStart {
                scale: Some((NoteName::C, ScaleType::Major))
            }
        );
    }

    /// clip-local `[bars N] [scale d minor]` 順でも scale が拾える (後勝ち)
    #[test]
    fn ctx_pitched_clip_line_start_with_bars_then_scale_d_minor() {
        let src = "clip x [bars 8] [scale d minor] {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipLineStart {
                scale: Some((NoteName::D, ScaleType::Minor))
            }
        );
    }

    /// 同じ clip ヘッダに複数 `[scale ...]` がある場合は後勝ち
    #[test]
    fn ctx_pitched_clip_line_start_last_scale_wins() {
        let src = "clip x [scale c major] [scale a minor] {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipLineStart {
                scale: Some((NoteName::A, ScaleType::Minor))
            }
        );
    }

    /// `chord ` のように instrument 名 + 空白が書かれた状態は
    /// PitchedClipAfterInstrument (音名/コードを期待) を返す。
    /// instrument 名が混ざるのは行頭のみで、ここでは出さない。
    #[test]
    fn ctx_pitched_clip_after_instrument_when_instrument_then_space() {
        let src = "clip x [scale d minor] {\n  chord ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipAfterInstrument {
                scale: Some((NoteName::D, ScaleType::Minor))
            }
        );
    }

    /// 音名を 1 つ書いた後 (`chord dm:4:1 `) も PitchedClipAfterInstrument のまま。
    /// 同じ instrument 名上で複数音/複数コードが続く文法に対応する。
    #[test]
    fn ctx_pitched_clip_after_instrument_continues_for_subsequent_tokens() {
        let src = "clip x [scale d minor] {\n  chord dm:4:1 ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipAfterInstrument {
                scale: Some((NoteName::D, ScaleType::Minor))
            }
        );
    }

    /// instrument 名を書き途中 (`cho`) は行頭扱いのまま (まだ空白を越えていない)。
    /// この段階では instrument 名候補の絞り込みを期待する。
    #[test]
    fn ctx_pitched_clip_line_start_while_typing_instrument_name() {
        let src = "clip x [scale d minor] {\n  cho";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipLineStart {
                scale: Some((NoteName::D, ScaleType::Minor))
            }
        );
    }

    #[test]
    fn ctx_drum_clip_body() {
        let src = "clip drums_a [bars 1] {\n  use tr808\n  resolution 16\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::DrumClipBody
        );
    }

    #[test]
    fn ctx_clip_after_use() {
        let src = "clip drums_a [bars 1] {\n  use ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipAfterUse
        );
    }

    #[test]
    fn ctx_scene_body() {
        let src = "scene intro {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::SceneBody
        );
    }

    #[test]
    fn ctx_scene_after_tempo() {
        let src = "scene buildup {\n  tempo ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterTempo
        );
    }

    #[test]
    fn ctx_session_body() {
        let src = "session main {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::SessionBody
        );
    }

    #[test]
    fn ctx_session_after_bracket() {
        let src = "session main {\n  intro [";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::SessionAfterBracket
        );
    }

    #[test]
    fn ctx_clip_option() {
        // トップレベルで "clip name [" → ClipOption（使用済みなし）
        let src = "clip bass_a [";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOption {
                used_options: vec![]
            }
        );
    }

    #[test]
    fn ctx_clip_option_inside_body() {
        // 前の clip が閉じた後、新しい clip のトップレベル "[" → ClipOption（使用済みなし）
        let src = "clip bass_a [bars 1] {\n  bass c:3:8\n}\nclip lead_a [";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOption {
                used_options: vec![]
            }
        );
    }

    #[test]
    fn ctx_toplevel_clip_option_scale() {
        // トップレベルで "clip name [scale " → ClipOptionAfterScale
        let src = "clip bass_a [scale ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOptionAfterScale
        );
    }

    #[test]
    fn ctx_toplevel_clip_option_scale_note() {
        // トップレベルで "clip name [scale c " → ClipOptionAfterScaleNote
        let src = "clip bass_a [scale c ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOptionAfterScaleNote
        );
    }

    #[test]
    fn ctx_toplevel_clip_after_closed_bracket() {
        // ブラケットが閉じている場合は AfterBlockKeyword
        let src = "clip bass_a [bars 4] ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterBlockKeyword
        );
    }

    #[test]
    fn ctx_toplevel_clip_option_after_bars() {
        // [bars 4] の後に [ → bars が除外される
        let src = "clip bass_a [bars 4] [";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOption {
                used_options: vec!["bars".to_string()]
            }
        );
    }

    #[test]
    fn ctx_toplevel_clip_option_after_scale() {
        // [scale c major] の後に [ → scale が除外される
        let src = "clip bass_a [scale c major] [";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOption {
                used_options: vec!["scale".to_string()]
            }
        );
    }

    #[test]
    fn ctx_toplevel_clip_option_after_bars_and_scale() {
        // [bars 4] [scale c major] の後に [ → bars と scale が除外される
        let src = "clip bass_a [bars 4] [scale c major] [";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::ClipOption {
                used_options: vec!["bars".to_string(), "scale".to_string()]
            }
        );
    }

    // --- word_at_offset tests ---

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

    // --- DeviceAfterPort context tests ---

    #[test]
    fn device_body_after_port_keyword_is_device_after_port() {
        // device foo {
        //   port <カーソル>
        //
        let src = "device foo {\n  port \n}";
        let offset = src.find("port ").unwrap() + "port ".len();
        let ctx = determine_completion_context(src, offset);
        assert_eq!(ctx, CompletionContext::DeviceAfterPort);
    }

    #[test]
    fn device_body_at_line_start_is_device_body() {
        let src = "device foo {\n  \n}";
        let offset = src.find("\n  ").unwrap() + 3; // 行頭の空白後
        let ctx = determine_completion_context(src, offset);
        assert_eq!(ctx, CompletionContext::DeviceBody);
    }

    #[test]
    fn device_body_with_partial_port_keyword_is_device_body() {
        // "p" だけ書いた状態は DeviceBody（port キーワード補完が出る位置）
        let src = "device foo {\n  p\n}";
        let offset = src.find("\n  p").unwrap() + 4; // "p" の直後
        let ctx = determine_completion_context(src, offset);
        assert_eq!(ctx, CompletionContext::DeviceBody);
    }

    /// `arp(` 直後では AfterArpOpen コンテキストになる。
    /// After `arp(` the context must be AfterArpOpen.
    #[test]
    fn ctx_after_arp_open() {
        let src = "clip arp_clip [bars 1] {\n  bass cm arp(";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterArpOpen
        );
    }

    /// `arp(   ` のような空白後でも AfterArpOpen のまま。
    /// AfterArpOpen still applies after whitespace.
    #[test]
    fn ctx_after_arp_open_with_whitespace() {
        let src = "clip arp_clip [bars 1] {\n  bass cm arp(  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterArpOpen
        );
    }

    /// `arp(up, ` 直後では AfterArpComma コンテキストになる。
    /// After `arp(<dir>, ` the context must be AfterArpComma.
    #[test]
    fn ctx_after_arp_comma() {
        let src = "clip arp_clip [bars 1] {\n  bass cm arp(up, ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterArpComma
        );
    }

    /// `arp(random,` のように空白なしカンマ直後でも AfterArpComma。
    /// AfterArpComma applies even without trailing space.
    #[test]
    fn ctx_after_arp_comma_no_space() {
        let src = "clip arp_clip [bars 1] {\n  bass cm arp(random,";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::AfterArpComma
        );
    }

    /// §8.6: SceneBody 文脈の補完候補に `mute` キーワードが含まれる
    /// §8.6: SceneBody completions include the `mute` keyword for scene-internal initial mute
    #[test]
    fn build_completion_items_for_scene_body_includes_mute_keyword() {
        let ctx = CompletionContext::SceneBody;
        let registry = Registry::new();
        let items = build_completion_items(&ctx, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"mute"),
            "SceneBody completions should include `mute` (§8.6). got: {labels:?}"
        );
        assert!(
            labels.contains(&"tempo"),
            "SceneBody completions should still include `tempo`. got: {labels:?}"
        );
    }

    /// AfterArpOpen の補完候補は up/down/updown/random の 4 件。
    /// AfterArpOpen returns the four direction completions.
    #[test]
    fn build_completion_items_for_after_arp_open_returns_directions() {
        let ctx = CompletionContext::AfterArpOpen;
        let registry = Registry::new();
        let items = build_completion_items(&ctx, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"up"));
        assert!(labels.contains(&"down"));
        assert!(labels.contains(&"updown"));
        assert!(labels.contains(&"random"));
        assert_eq!(labels.len(), 4);
    }

    /// AfterArpComma の補完候補は主要音価 (4, 8, 16, 32)。
    /// AfterArpComma returns the major note resolutions.
    #[test]
    fn build_completion_items_for_after_arp_comma_returns_resolutions() {
        let ctx = CompletionContext::AfterArpComma;
        let registry = Registry::new();
        let items = build_completion_items(&ctx, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"4"));
        assert!(labels.contains(&"8"));
        assert!(labels.contains(&"16"));
        assert!(labels.contains(&"32"));
    }

    /// 行頭 (PitchedClipLineStart) の候補は **registry に登録された
    /// instrument 名のみ**。音名・コード名・半音階フォールバックは含まない。
    /// At the line start, completions are exclusively instrument identifiers.
    #[test]
    fn build_pitched_clip_line_start_returns_only_instrument_names() {
        use crate::ast::device::DeviceDef;
        use crate::ast::instrument::InstrumentDef;
        use crate::ast::Block;
        use crate::domain::channel::MidiChannel;

        let ctx = CompletionContext::PitchedClipLineStart {
            scale: Some((NoteName::D, ScaleType::Minor)),
        };
        let mut registry = Registry::new();
        registry.register_block(Block::Device(DeviceDef {
            name: "dev".to_string(),
            port: "p".to_string(),
            transport: true,
        }));
        for name in ["chord", "lead", "bass"] {
            registry.register_block(Block::Instrument(InstrumentDef {
                name: name.to_string(),
                device: "dev".to_string(),
                channel: MidiChannel::from_one_based(1).unwrap(),
                note: None,
                gate_normal: None,
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                cc_mappings: vec![],
                local_vars: vec![],
                unresolved: Default::default(),
            }));
        }
        let items = build_completion_items(&ctx, &registry);

        // すべて instrument 識別子で構成される
        let labels: std::collections::HashSet<&str> =
            items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains("chord"));
        assert!(labels.contains("lead"));
        assert!(labels.contains("bass"));
        // 音名/コード名/半音階フォールバックは出ない
        assert!(items.iter().all(|i| i.kind != CompletionKind::NoteName));
        assert!(items.iter().all(|i| i.kind != CompletionKind::ChordName));
    }

    /// `chord ` の続き (PitchedClipAfterInstrument) では
    /// **instrument 名は候補に含めない**。これが本修正の本丸。
    /// After the instrument token, instrument names must not be suggested.
    #[test]
    fn build_pitched_clip_after_instrument_excludes_instrument_names() {
        use crate::ast::device::DeviceDef;
        use crate::ast::instrument::InstrumentDef;
        use crate::ast::Block;
        use crate::domain::channel::MidiChannel;

        let ctx = CompletionContext::PitchedClipAfterInstrument {
            scale: Some((NoteName::D, ScaleType::Minor)),
        };
        let mut registry = Registry::new();
        registry.register_block(Block::Device(DeviceDef {
            name: "dev".to_string(),
            port: "p".to_string(),
            transport: true,
        }));
        for name in ["chord", "lead", "bass"] {
            registry.register_block(Block::Instrument(InstrumentDef {
                name: name.to_string(),
                device: "dev".to_string(),
                channel: MidiChannel::from_one_based(1).unwrap(),
                note: None,
                gate_normal: None,
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                cc_mappings: vec![],
                local_vars: vec![],
                unresolved: Default::default(),
            }));
        }
        let items = build_completion_items(&ctx, &registry);
        let labels: std::collections::HashSet<&str> =
            items.iter().map(|i| i.label.as_str()).collect();
        for name in ["chord", "lead", "bass"] {
            assert!(
                !labels.contains(name),
                "instrument name `{name}` must not appear after the instrument token"
            );
        }
        // 代わりに音名/コードは出る
        assert!(items.iter().any(|i| i.kind == CompletionKind::NoteName));
        assert!(items.iter().any(|i| i.kind == CompletionKind::ChordName));
    }

    /// instrument 名直後 (clip-local scale) の候補はスケール構成音 7 音と
    /// ダイアトニックコード 7 つだけで構成され、半音階フォールバックは含まれない。
    #[test]
    fn build_pitched_clip_after_instrument_with_clip_local_scale_includes_scale_notes_and_diatonic()
    {
        let ctx = CompletionContext::PitchedClipAfterInstrument {
            scale: Some((NoteName::C, ScaleType::Major)),
        };
        let registry = Registry::new();
        let items = build_completion_items(&ctx, &registry);

        // c major 構成音 (sortText "0_*") が7つ
        let scale_notes: Vec<&str> = items
            .iter()
            .filter(|i| i.sort_text.as_deref().is_some_and(|s| s.starts_with("0_")))
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(scale_notes, vec!["c", "d", "e", "f", "g", "a", "b"]);

        // ダイアトニックコード (ChordName) が7つ
        let chords: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == CompletionKind::ChordName)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(
            chords.len(),
            7,
            "diatonic chords expected 7, got {chords:?}"
        );

        // 半音階フォールバック (sortText "9_*") は出さない
        let chromatic_count = items
            .iter()
            .filter(|i| i.sort_text.as_deref().is_some_and(|s| s.starts_with("9_")))
            .count();
        assert_eq!(
            chromatic_count, 0,
            "scale が解決できているときは半音階フォールバックを出さない"
        );

        // NoteName kind の候補もスケール構成音 7 つだけ
        let note_kind_count = items
            .iter()
            .filter(|i| i.kind == CompletionKind::NoteName)
            .count();
        assert_eq!(
            note_kind_count, 7,
            "scale 解決時の NoteName 候補は構成音 7 つのみ"
        );
    }

    /// clip-local scale が無い場合は registry (top-level scale) にフォールバックする。
    /// (top-level scale が解決できているので半音階フォールバックは出ない)
    #[test]
    fn build_pitched_clip_after_instrument_falls_back_to_registry_scale() {
        use crate::ast::scale::ScaleDef;
        use crate::ast::Block;

        let ctx = CompletionContext::PitchedClipAfterInstrument { scale: None };
        let mut registry = Registry::new();
        registry.register_block(Block::Scale(ScaleDef {
            root: NoteName::A,
            scale_type: ScaleType::Minor,
        }));
        let items = build_completion_items(&ctx, &registry);

        // a minor 構成音: a b c d e f g
        let scale_notes: Vec<&str> = items
            .iter()
            .filter(|i| i.sort_text.as_deref().is_some_and(|s| s.starts_with("0_")))
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(scale_notes, vec!["a", "b", "c", "d", "e", "f", "g"]);

        // 半音階フォールバック (sortText "9_*") は出さない
        let chromatic_count = items
            .iter()
            .filter(|i| i.sort_text.as_deref().is_some_and(|s| s.starts_with("9_")))
            .count();
        assert_eq!(chromatic_count, 0);
    }

    /// scale が両方無い場合は半音階17音のみで sortText も付かない。
    #[test]
    fn build_pitched_clip_after_instrument_without_any_scale_uses_chromatic_only() {
        let ctx = CompletionContext::PitchedClipAfterInstrument { scale: None };
        let registry = Registry::new();
        let items = build_completion_items(&ctx, &registry);

        // ChordName / "0_*" sortText は無い
        assert!(items.iter().all(|i| i.kind != CompletionKind::ChordName));
        assert!(items.iter().all(|i| i
            .sort_text
            .as_deref()
            .map(|s| !s.starts_with("0_"))
            .unwrap_or(true)));

        // NoteName が17件揃う (sort_text なし)
        let chromatic: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == CompletionKind::NoteName)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(chromatic.len(), 17);
        for item in items.iter().filter(|i| i.kind == CompletionKind::NoteName) {
            assert!(item.sort_text.is_none());
        }
    }

    /// clip-local scale は registry の top-level scale を上書きする。
    #[test]
    fn build_pitched_clip_after_instrument_clip_local_scale_overrides_registry() {
        use crate::ast::scale::ScaleDef;
        use crate::ast::Block;

        let ctx = CompletionContext::PitchedClipAfterInstrument {
            scale: Some((NoteName::C, ScaleType::Major)),
        };
        let mut registry = Registry::new();
        registry.register_block(Block::Scale(ScaleDef {
            root: NoteName::C,
            scale_type: ScaleType::Minor,
        }));
        let items = build_completion_items(&ctx, &registry);

        // clip-local の c major が反映されている (eb ではなく e が出る)
        let scale_notes: Vec<&str> = items
            .iter()
            .filter(|i| i.sort_text.as_deref().is_some_and(|s| s.starts_with("0_")))
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(scale_notes, vec!["c", "d", "e", "f", "g", "a", "b"]);
    }

    #[test]
    fn build_completion_items_for_device_after_port_returns_midi_ports_or_empty() {
        // CI 環境では ALSA が見えず list_ports() が Err になることがあるため、
        // 結果は「Vec<CompletionItem> として正しく返る」までを検証する。
        // 環境に依存して空 or 非空のどちらも許容する。
        //
        // In CI environments without ALSA, `list_ports()` may fail; we only
        // verify that the result is a well-formed `Vec<CompletionItem>`, and
        // tolerate both empty and non-empty results depending on the host.
        let ctx = CompletionContext::DeviceAfterPort;
        let registry = Registry::new();
        let items = build_completion_items(&ctx, &registry);

        // 全アイテムが Identifier kind かつ detail が "MIDI port" であること
        // Every returned item must be an Identifier whose detail is "MIDI port".
        for item in &items {
            assert_eq!(item.kind, CompletionKind::Identifier);
            assert_eq!(item.detail.as_deref(), Some("MIDI port"));
        }
    }
}
