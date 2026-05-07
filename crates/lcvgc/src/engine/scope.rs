//! 変数スコープチェーン
//!
//! グローバル（トップレベル）とブロック（`{}` 内）の2段スコープを管理する。
//! 内側が優先（シャドーイング）。
//!
//! Variable scope chain
//!
//! Manages a two-level scope: global (top-level) and block (`{}` inner).
//! Inner scope takes priority (shadowing).

use std::collections::HashMap;

/// 変数スコープチェーン
/// Variable scope chain
///
/// `Vec<HashMap>` のスタックでスコープを管理する。
/// インデックス0がグローバルスコープ、push_scope で新しいブロックスコープを追加し、
/// pop_scope で除去する。resolve は内側（末尾）から外側（先頭）へ探索する。
/// Manages scopes with a `Vec<HashMap>` stack.
/// Index 0 is the global scope. `push_scope` adds a new block scope,
/// `pop_scope` removes it. `resolve` searches from inner (tail) to outer (head).
#[derive(Debug, Clone)]
pub struct ScopeChain {
    /// スコープスタック（インデックス0 = グローバル）
    /// Scope stack (index 0 = global)
    scopes: Vec<HashMap<String, String>>,
}

impl ScopeChain {
    /// グローバルスコープのみで初期化する
    /// Initialize with only the global scope
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// 新しいブロックスコープをプッシュする
    /// Push a new block scope
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 最上位のブロックスコープをポップする
    /// Pop the top block scope
    ///
    /// グローバルスコープ（インデックス0）はポップしない。
    /// The global scope (index 0) is never popped.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// 現在のスコープ（最上位）に変数を定義する
    /// Define a variable in the current (top) scope
    pub fn define(&mut self, name: String, value: String) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    /// グローバルスコープに変数を定義する
    /// Define a variable in the global scope
    pub fn define_global(&mut self, name: String, value: String) {
        self.scopes[0].insert(name, value);
    }

    /// 内側から外側へ探索して変数を解決する
    /// Resolve a variable by searching from inner to outer scope
    pub fn resolve(&self, name: &str) -> Option<&str> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value.as_str());
            }
        }
        None
    }

    /// 現在のスコープの深さを返す（1 = グローバルのみ）
    /// Return the current scope depth (1 = global only)
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }

    /// グローバルスコープの全変数名を返す
    /// Return all variable names in the global scope
    pub fn global_var_names(&self) -> Vec<String> {
        self.scopes[0].keys().cloned().collect()
    }

    /// グローバルスコープの変数を取得する
    /// Get a variable from the global scope
    pub fn get_global_var(&self, name: &str) -> Option<&str> {
        self.scopes[0].get(name).map(|s| s.as_str())
    }

    /// グローバルスコープが空かどうか
    /// Whether the global scope is empty
    pub fn is_global_empty(&self) -> bool {
        self.scopes[0].is_empty()
    }
}

impl Default for ScopeChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// グローバルスコープでの define/resolve
    /// Define and resolve in global scope
    #[test]
    fn global_define_resolve() {
        let mut scope = ScopeChain::new();
        scope.define("dev".into(), "mutant_brain".into());
        assert_eq!(scope.resolve("dev"), Some("mutant_brain"));
    }

    /// 未定義変数は None を返す
    /// Undefined variable returns None
    #[test]
    fn undefined_returns_none() {
        let scope = ScopeChain::new();
        assert_eq!(scope.resolve("unknown"), None);
    }

    /// ブロックスコープでシャドーイング
    /// Block scope shadows global scope
    #[test]
    fn block_scope_shadows_global() {
        let mut scope = ScopeChain::new();
        scope.define("ch".into(), "1".into());

        scope.push_scope();
        scope.define("ch".into(), "3".into());
        assert_eq!(scope.resolve("ch"), Some("3"));

        scope.pop_scope();
        assert_eq!(scope.resolve("ch"), Some("1"));
    }

    /// ブロックスコープからグローバル変数へのフォールバック
    /// Block scope falls back to global variables
    #[test]
    fn block_scope_falls_back_to_global() {
        let mut scope = ScopeChain::new();
        scope.define("dev".into(), "mutant_brain".into());

        scope.push_scope();
        // ブロック内で dev を再定義しない
        assert_eq!(scope.resolve("dev"), Some("mutant_brain"));
        scope.pop_scope();
    }

    /// pop_scope 後にグローバル値が復帰する
    /// Global value is restored after pop_scope
    #[test]
    fn pop_restores_global() {
        let mut scope = ScopeChain::new();
        scope.define("x".into(), "global".into());

        scope.push_scope();
        scope.define("x".into(), "block".into());
        assert_eq!(scope.resolve("x"), Some("block"));

        scope.pop_scope();
        assert_eq!(scope.resolve("x"), Some("global"));
    }

    /// グローバルスコープは pop できない
    /// Global scope cannot be popped
    #[test]
    fn cannot_pop_global() {
        let mut scope = ScopeChain::new();
        scope.pop_scope();
        assert_eq!(scope.depth(), 1);
    }

    /// define_global はブロックスコープ中でもグローバルに定義する
    /// define_global defines in global scope even inside a block
    #[test]
    fn define_global_in_block() {
        let mut scope = ScopeChain::new();
        scope.push_scope();
        scope.define_global("dev".into(), "mb".into());
        scope.pop_scope();
        assert_eq!(scope.resolve("dev"), Some("mb"));
    }

    /// 深さの確認
    /// Depth check
    #[test]
    fn depth_tracking() {
        let mut scope = ScopeChain::new();
        assert_eq!(scope.depth(), 1);
        scope.push_scope();
        assert_eq!(scope.depth(), 2);
        scope.push_scope();
        assert_eq!(scope.depth(), 3);
        scope.pop_scope();
        assert_eq!(scope.depth(), 2);
    }

    /// 独立ブロック間でスコープが分離していること
    /// Scopes are isolated between independent blocks
    #[test]
    fn independent_blocks_isolated() {
        let mut scope = ScopeChain::new();
        scope.define("ch".into(), "1".into());

        // ブロック1
        scope.push_scope();
        scope.define("ch".into(), "3".into());
        assert_eq!(scope.resolve("ch"), Some("3"));
        scope.pop_scope();

        // ブロック2（ブロック1のローカル変数は見えない）
        scope.push_scope();
        assert_eq!(scope.resolve("ch"), Some("1"));
        scope.pop_scope();
    }
}
