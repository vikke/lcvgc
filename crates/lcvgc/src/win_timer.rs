//! Windows のシステムタイマー粒度を 1ms に上げる RAII ガード
//!
//! Windows のデフォルト timer resolution は約 15.6ms (= 64Hz) で、
//! tokio の `time::sleep` / `time::interval` は OS のタイマー粒度に丸められる。
//! lcvgc の playback driver は BPM=240 (PPQ=480) なら 520us/tick で sleep を
//! 要求するが、デフォルト粒度のままでは全部 15.6ms に丸められ、BPM 60/120/240
//! どれでも実 interval が同じになり、tempo が音に効かなくなる。
//!
//! `HighResolutionTimer::acquire()` を main の最初で握っておくと、生存期間中
//! `timeBeginPeriod(1)` が有効化され、Drop 時に `timeEndPeriod(1)` で元に戻す。
//! Windows 以外のプラットフォームでは何もしない no-op 実装を提供する。
//!
//! On Windows the default timer resolution is ~15.6ms, which causes tokio
//! sleeps shorter than that (e.g. the 520us tick at BPM=240 with PPQ=480) to
//! be rounded up to ~15.6ms. That makes every BPM in the sub-15.6ms range
//! play back at the same speed. This RAII guard calls `timeBeginPeriod(1)`
//! at construction and `timeEndPeriod(1)` on drop so the system timer runs
//! at 1ms granularity for the lifetime of the lcvgc process. On non-Windows
//! targets the guard is a no-op.

/// 高解像度タイマー要求の RAII ガード
///
/// Windows では `timeBeginPeriod(1)` を呼んだ状態を生存期間中保持し、
/// Drop で `timeEndPeriod(1)` を呼んで元に戻す。Windows 以外では何もしない。
///
/// RAII guard that requests a 1ms system timer resolution on Windows for
/// its lifetime. No-op on other platforms.
pub struct HighResolutionTimer {
    // フィールドは中身を持たないが、Drop を実装するために型自体は必要。
    // The field-less type still needs to exist so we can implement Drop.
    _private: (),
}

impl HighResolutionTimer {
    /// 高解像度タイマーを獲得する
    ///
    /// Windows では `timeBeginPeriod(1)` を呼び、それ以外のプラットフォームでは
    /// 何もしない。返り値の `HighResolutionTimer` を main 内で hold すると、
    /// プロセス終了 (panic 含む) まで 1ms 粒度が維持される。
    ///
    /// Acquires the high-resolution timer. On Windows this calls
    /// `timeBeginPeriod(1)`; elsewhere it is a no-op. Hold the returned
    /// guard in `main` so the resolution stays elevated until the process
    /// exits (including via panic).
    pub fn acquire() -> Self {
        #[cfg(windows)]
        {
            // SAFETY: timeBeginPeriod は Windows 公開 API。1ms はサポートされる
            // 最小値で、対応する timeEndPeriod を Drop で呼ぶ。
            // SAFETY: timeBeginPeriod is a public Windows API. 1ms is the
            // minimum supported resolution, paired with timeEndPeriod in Drop.
            let rc = unsafe { windows_sys::Win32::Media::timeBeginPeriod(1) };
            if rc == windows_sys::Win32::Media::TIMERR_NOERROR {
                tracing::info!("Windows timer resolution を 1ms に変更しました");
            } else {
                tracing::warn!(
                    "timeBeginPeriod(1) が失敗しました (rc={}). tempo 精度が劣化する可能性があります",
                    rc
                );
            }
        }
        Self { _private: () }
    }
}

impl Drop for HighResolutionTimer {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            // SAFETY: acquire 時に timeBeginPeriod(1) を呼んでおり、対応する
            // timeEndPeriod(1) を呼んで元の粒度に戻す。
            // SAFETY: paired with the timeBeginPeriod(1) call in acquire().
            unsafe {
                windows_sys::Win32::Media::timeEndPeriod(1);
            }
        }
    }
}
