use super::completion::{CompletionItem, CompletionKind, CompletionProvider};
use crate::engine::registry::Registry;

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
    /// clip ブロック内の行頭（pitched）: 楽器名を提案
    PitchedClipBody,
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

    if trimmed.is_empty() {
        if clip_has_use(source, brace_pos, cursor_offset) {
            return CompletionContext::DrumClipBody;
        }
        return CompletionContext::PitchedClipBody;
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
        CompletionContext::PitchedClipBody
    }
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

        // 実装は list_ports 補完 agent が差し替える
        // To be replaced by the list_ports completion agent
        CompletionContext::DeviceAfterPort => vec![],

        CompletionContext::InstrumentBody => CompletionProvider::instrument_body_completions(),

        CompletionContext::InstrumentAfterDevice => {
            CompletionProvider::identifier_completions(&registry.device_names(), "device")
        }

        CompletionContext::InstrumentAfterNote => CompletionProvider::note_completions(),

        CompletionContext::KitBody => CompletionProvider::kit_body_completions(),

        CompletionContext::KitAfterDevice => {
            CompletionProvider::identifier_completions(&registry.device_names(), "device")
        }

        CompletionContext::PitchedClipBody => {
            let mut items = CompletionProvider::identifier_completions(
                &registry.instrument_names(),
                "instrument",
            );
            items.extend(CompletionProvider::note_completions());
            // ダイアトニックコード（scale設定がある場合）
            if let Some(scale) = registry.scale() {
                items.extend(CompletionProvider::diatonic_completions(
                    scale.root,
                    scale.scale_type,
                ));
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
                        detail: Some(format!("kit instrument (ch{})", inst.channel)),
                        kind: CompletionKind::Identifier,
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

    #[test]
    fn ctx_pitched_clip_body() {
        let src = "clip bass_a [bars 1] {\n  ";
        assert_eq!(
            determine_completion_context(src, src.len()),
            CompletionContext::PitchedClipBody
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
}
