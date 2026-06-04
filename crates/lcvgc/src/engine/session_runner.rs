use crate::ast::session::{SessionDef, SessionEntry, SessionRepeat};

/// セッション進行時のアクション
/// Action during session progression
#[derive(Debug, Clone, PartialEq)]
pub enum SessionAction {
    /// 次のシーンを再生
    /// Play the next scene
    PlayScene(String),
    /// セッション完了
    /// Session completed
    Done,
}

/// セッション再生シーケンスを管理
/// Manages the session playback sequence
///
/// セッション内のエントリを順番に処理し、リピート制御を行う。
/// Processes entries within a session sequentially and handles repeat control.
#[derive(Debug)]
pub struct SessionRunner {
    /// セッション内のエントリ一覧
    /// List of entries within the session
    entries: Vec<SessionEntry>,
    /// 現在処理中のエントリインデックス
    /// Index of the currently processing entry
    current_index: usize,
    /// 現在のエントリの残りリピート回数（None = Loop無限）
    /// Remaining repeat count for the current entry (None = infinite loop)
    current_repeat_remaining: Option<u32>,
    /// セッション全体をループするか
    /// Whether to loop the entire session
    session_looping: bool,
    /// 直前の advance でエントリ境界を越えたか（§12 session 上書き検出用）
    /// Whether the last advance crossed an entry boundary (for §12 session overwrite detection)
    last_advance_crossed_entry: bool,
}

impl SessionRunner {
    /// セッション定義からランナーを作成する
    /// Creates a runner from a session definition
    pub fn new(session: &SessionDef) -> Self {
        let repeat_remaining = session
            .entries
            .first()
            .map(|e| Self::initial_remaining(&e.repeat));

        Self {
            entries: session.entries.clone(),
            current_index: 0,
            current_repeat_remaining: repeat_remaining.unwrap_or(Some(0)),
            session_looping: false,
            last_advance_crossed_entry: false,
        }
    }

    /// セッション全体のループを設定して作成する
    /// Creates a runner with session-wide looping enabled
    pub fn new_looping(session: &SessionDef) -> Self {
        let mut runner = Self::new(session);
        runner.session_looping = true;
        runner
    }

    /// 次のアクションを返す。シーンの1サイクル完了ごとに呼ぶ。
    /// Returns the next action. Called after each scene cycle completes.
    pub fn advance(&mut self) -> SessionAction {
        self.last_advance_crossed_entry = false;

        // エントリが空または末尾を超えた場合
        if self.current_index >= self.entries.len() {
            if self.session_looping && !self.entries.is_empty() {
                self.reset();
                self.last_advance_crossed_entry = true;
            } else {
                return SessionAction::Done;
            }
        }

        let entry = &self.entries[self.current_index];
        let scene_name = entry.scene.clone();

        match entry.repeat {
            SessionRepeat::Once => {
                self.current_index += 1;
                self.init_current_repeat();
                self.last_advance_crossed_entry = true;
                SessionAction::PlayScene(scene_name)
            }
            SessionRepeat::Count(_) => {
                match &mut self.current_repeat_remaining {
                    Some(remaining) if *remaining > 1 => {
                        *remaining -= 1;
                    }
                    _ => {
                        self.current_index += 1;
                        self.init_current_repeat();
                        self.last_advance_crossed_entry = true;
                    }
                }
                SessionAction::PlayScene(scene_name)
            }
            SessionRepeat::Loop => {
                // Loop は常に同じシーンを再生し、進まない
                SessionAction::PlayScene(scene_name)
            }
        }
    }

    /// 現エントリの残りリピートを捨てて次のエントリへ強制的に進める
    ///
    /// 更新起因（clip / scene / session 定義の上書き）の4小節グリッド強制遷移で
    /// 使う。`advance()` がリピートモードに従って現エントリを再生し続けるのに対し、
    /// 本メソッドは Count の残りや Loop に関わらず必ず `current_index` を1つ進め、
    /// 進んだ先のエントリの scene を返す。末尾を越えた場合、全体ループ中なら先頭へ
    /// 折り返し、そうでなければ `Done` を返す。常に `last_advance_crossed_entry` を
    /// true にする（エントリ境界を越えたとみなす）。
    ///
    /// Forcibly advances to the next entry, discarding the current entry's
    /// remaining repeats. Used by the update-triggered 4-bar-grid forced
    /// transition. Unlike `advance()`, which keeps replaying the current entry
    /// per its repeat mode, this always moves `current_index` forward by one and
    /// returns the scene of the entry it lands on. When past the end it wraps to
    /// the start if session-looping, otherwise returns `Done`. Always sets
    /// `last_advance_crossed_entry` to true.
    ///
    /// # Returns
    /// 進んだ先エントリの `SessionAction::PlayScene`、または末尾超過時の
    /// `SessionAction::Done`
    pub fn force_next_entry(&mut self) -> SessionAction {
        self.last_advance_crossed_entry = true;
        self.current_index += 1;

        if self.current_index >= self.entries.len() {
            if self.session_looping && !self.entries.is_empty() {
                self.reset();
            } else {
                return SessionAction::Done;
            }
        }

        self.init_current_repeat();
        let scene_name = self.entries[self.current_index].scene.clone();
        SessionAction::PlayScene(scene_name)
    }

    /// 直前の advance() がエントリ境界を越えたか（§12 上書き検出用）
    /// Whether the last advance() crossed an entry boundary (for §12 overwrite detection)
    pub fn last_advance_crossed_entry(&self) -> bool {
        self.last_advance_crossed_entry
    }

    /// エントリ数
    /// Number of entries
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 先頭にリセット
    /// Resets to the beginning
    pub fn reset(&mut self) {
        self.current_index = 0;
        self.init_current_repeat();
    }

    /// 完了しているか
    /// Returns whether the session has completed
    pub fn is_done(&self) -> bool {
        self.current_index >= self.entries.len()
    }

    /// 現在のエントリインデックス
    /// Returns the current entry index
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// リピートモードから初期残り回数を計算する
    /// Calculates the initial remaining count from a repeat mode
    fn initial_remaining(repeat: &SessionRepeat) -> Option<u32> {
        match repeat {
            SessionRepeat::Once => Some(1),
            SessionRepeat::Count(n) => Some(*n),
            SessionRepeat::Loop => None,
        }
    }

    /// 現在のエントリのリピート残り回数を初期化する
    /// Initializes the remaining repeat count for the current entry
    fn init_current_repeat(&mut self) {
        self.current_repeat_remaining = self
            .entries
            .get(self.current_index)
            .map(|e| Self::initial_remaining(&e.repeat))
            .unwrap_or(Some(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(entries: Vec<SessionEntry>) -> SessionDef {
        SessionDef {
            name: "test".to_string(),
            entries,
        }
    }

    fn entry(scene: &str, repeat: SessionRepeat) -> SessionEntry {
        SessionEntry {
            scene: scene.to_string(),
            repeat,
        }
    }

    // 1. 空のセッション → advance は即座に Done を返す
    #[test]
    fn empty_session_returns_done() {
        let s = session(vec![]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(runner.advance(), SessionAction::Done);
    }

    // 2. 単一シーン Once → PlayScene 1回、その後 Done
    #[test]
    fn single_scene_once() {
        let s = session(vec![entry("intro", SessionRepeat::Once)]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("intro".to_string())
        );
        assert_eq!(runner.advance(), SessionAction::Done);
    }

    // 3. 単一シーン Count(3) → PlayScene 3回、その後 Done
    #[test]
    fn single_scene_count_3() {
        let s = session(vec![entry("verse", SessionRepeat::Count(3))]);
        let mut runner = SessionRunner::new(&s);
        for _ in 0..3 {
            assert_eq!(
                runner.advance(),
                SessionAction::PlayScene("verse".to_string())
            );
        }
        assert_eq!(runner.advance(), SessionAction::Done);
    }

    // 4. 単一シーン Loop → PlayScene を繰り返し、Done にならない
    #[test]
    fn single_scene_loop_never_done() {
        let s = session(vec![entry("ambient", SessionRepeat::Loop)]);
        let mut runner = SessionRunner::new(&s);
        for _ in 0..100 {
            assert_eq!(
                runner.advance(),
                SessionAction::PlayScene("ambient".to_string())
            );
        }
        assert!(!runner.is_done());
    }

    // 5. 複数シーン Once → 順番に再生されてから Done
    #[test]
    fn multiple_scenes_once() {
        let s = session(vec![
            entry("intro", SessionRepeat::Once),
            entry("verse", SessionRepeat::Once),
            entry("chorus", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("intro".to_string())
        );
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("verse".to_string())
        );
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("chorus".to_string())
        );
        assert_eq!(runner.advance(), SessionAction::Done);
    }

    // 6. 混合リピートモード
    #[test]
    fn mixed_repeat_modes() {
        let s = session(vec![
            entry("intro", SessionRepeat::Once),
            entry("verse", SessionRepeat::Count(2)),
            entry("outro", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("intro".to_string())
        );
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("verse".to_string())
        );
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("verse".to_string())
        );
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("outro".to_string())
        );
        assert_eq!(runner.advance(), SessionAction::Done);
    }

    // 7. reset で先頭に戻る
    #[test]
    fn reset_returns_to_start() {
        let s = session(vec![
            entry("a", SessionRepeat::Once),
            entry("b", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(runner.advance(), SessionAction::PlayScene("a".to_string()));
        assert_eq!(runner.advance(), SessionAction::PlayScene("b".to_string()));
        assert_eq!(runner.advance(), SessionAction::Done);

        runner.reset();
        assert!(!runner.is_done());
        assert_eq!(runner.advance(), SessionAction::PlayScene("a".to_string()));
    }

    // 8. is_done は完了後に true を返す
    #[test]
    fn is_done_after_completion() {
        let s = session(vec![entry("x", SessionRepeat::Once)]);
        let mut runner = SessionRunner::new(&s);
        assert!(!runner.is_done());
        runner.advance();
        assert!(runner.is_done());
    }

    // 9. current_index のトラッキング
    #[test]
    fn current_index_tracking() {
        let s = session(vec![
            entry("a", SessionRepeat::Once),
            entry("b", SessionRepeat::Count(2)),
            entry("c", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(runner.current_index(), 0);

        runner.advance(); // a (Once) → index moves to 1
        assert_eq!(runner.current_index(), 1);

        runner.advance(); // b (Count 2, first) → still at 1
        assert_eq!(runner.current_index(), 1);

        runner.advance(); // b (Count 2, second) → moves to 2
        assert_eq!(runner.current_index(), 2);

        runner.advance(); // c (Once) → moves to 3
        assert_eq!(runner.current_index(), 3);
    }

    // 10. Count(1) は Once と同じ動作
    #[test]
    fn count_1_same_as_once() {
        let s_once = session(vec![entry("x", SessionRepeat::Once)]);
        let s_count = session(vec![entry("x", SessionRepeat::Count(1))]);

        let mut runner_once = SessionRunner::new(&s_once);
        let mut runner_count = SessionRunner::new(&s_count);

        assert_eq!(runner_once.advance(), runner_count.advance());
        assert_eq!(runner_once.advance(), runner_count.advance());
    }

    // 11. セッション全体ループ → Done の代わりに先頭に戻る
    #[test]
    fn session_looping() {
        let s = session(vec![
            entry("a", SessionRepeat::Once),
            entry("b", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new_looping(&s);

        assert_eq!(runner.advance(), SessionAction::PlayScene("a".to_string()));
        assert_eq!(runner.advance(), SessionAction::PlayScene("b".to_string()));
        // ループで先頭に戻る
        assert_eq!(runner.advance(), SessionAction::PlayScene("a".to_string()));
        assert_eq!(runner.advance(), SessionAction::PlayScene("b".to_string()));
    }

    // 12. 空セッションの is_done は true
    #[test]
    fn empty_session_is_done() {
        let s = session(vec![]);
        let runner = SessionRunner::new(&s);
        assert!(runner.is_done());
    }

    // 13. force_next_entry: Count(N) の途中でも残りを捨てて次エントリへ進む
    //     (更新起因の4小節グリッド強制遷移)
    // force_next_entry: discards remaining repeats mid-Count and jumps to the
    // next entry (update-triggered 4-bar-grid forced transition).
    #[test]
    fn force_next_entry_skips_remaining_count() {
        let s = session(vec![
            entry("a", SessionRepeat::Count(4)),
            entry("b", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new(&s);
        // 1 回だけ a を再生した時点（残り Count あり）で強制遷移
        assert_eq!(runner.advance(), SessionAction::PlayScene("a".to_string()));
        assert_eq!(runner.current_index(), 0);
        // force_next_entry は残りの a を捨てて次エントリ b を返す
        assert_eq!(
            runner.force_next_entry(),
            SessionAction::PlayScene("b".to_string())
        );
        assert_eq!(runner.current_index(), 1);
        assert!(runner.last_advance_crossed_entry());
    }

    // 14. force_next_entry: Loop エントリでも次エントリへ強制進行する
    // force_next_entry: a Loop entry is also forced to advance to the next entry.
    #[test]
    fn force_next_entry_breaks_loop_entry() {
        let s = session(vec![
            entry("loop_scene", SessionRepeat::Loop),
            entry("next", SessionRepeat::Once),
        ]);
        let mut runner = SessionRunner::new(&s);
        // Loop は advance では進まない
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("loop_scene".to_string())
        );
        assert_eq!(runner.current_index(), 0);
        // force_next_entry で次エントリへ抜ける
        assert_eq!(
            runner.force_next_entry(),
            SessionAction::PlayScene("next".to_string())
        );
        assert_eq!(runner.current_index(), 1);
    }

    // 15. force_next_entry: 末尾エントリで呼ぶと非ループ時は Done、
    //     全体ループ時は先頭へ戻る
    // force_next_entry at the last entry returns Done (non-looping) or wraps to
    // the first entry (session-looping).
    #[test]
    fn force_next_entry_at_last_entry() {
        // 非ループ: 末尾で force_next_entry → Done
        let s = session(vec![entry("only", SessionRepeat::Loop)]);
        let mut runner = SessionRunner::new(&s);
        assert_eq!(
            runner.advance(),
            SessionAction::PlayScene("only".to_string())
        );
        assert_eq!(runner.force_next_entry(), SessionAction::Done);
        assert!(runner.is_done());

        // 全体ループ: 末尾で force_next_entry → 先頭へ戻る
        let s2 = session(vec![
            entry("a", SessionRepeat::Loop),
            entry("b", SessionRepeat::Loop),
        ]);
        let mut runner2 = SessionRunner::new_looping(&s2);
        assert_eq!(runner2.advance(), SessionAction::PlayScene("a".to_string()));
        // a → b
        assert_eq!(
            runner2.force_next_entry(),
            SessionAction::PlayScene("b".to_string())
        );
        // b → 先頭 a へ折り返す
        assert_eq!(
            runner2.force_next_entry(),
            SessionAction::PlayScene("a".to_string())
        );
        assert!(runner2.last_advance_crossed_entry());
    }
}
