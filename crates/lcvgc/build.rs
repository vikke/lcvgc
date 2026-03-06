//! ビルドスクリプト: Gitハッシュをコンパイル時環境変数として埋め込む
//! Build script: embeds Git hash as a compile-time environment variable

use std::process::Command;

/// ビルド時にGitコミットハッシュを取得し、`GIT_HASH`環境変数として設定する
/// Retrieves the Git commit hash at build time and sets it as the `GIT_HASH` env var
fn main() {
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={hash}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");
}
