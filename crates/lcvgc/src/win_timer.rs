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
//!
//! さらに Windows 10 2004 以降では、フォアグラウンドでないプロセスに対し OS が
//! 自動的にタイマー解像度を引き下げる Power Throttling が働く。lcvgc.exe を
//! PowerShell から起動した状態で別ウィンドウ (例: WSL2 ターミナル上の nvim)
//! をフォアグラウンドにすると、要求した 1ms 粒度が剥がれ tempo が極端に
//! 遅くなる。これを防ぐため `acquire()` 内で
//! `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` も立て、本プロセスに
//! ついては OS の自動引き下げを抑止する。
//!
//! Additionally, since Windows 10 2004 the OS auto-throttles timer
//! resolution for non-foreground processes (Power Throttling). When the
//! lcvgc.exe window loses focus (e.g. switching to a WSL2 terminal running
//! nvim), the requested 1ms resolution is dropped and tempo slows down
//! dramatically. `acquire()` therefore also opts this process out of timer
//! resolution throttling via
//! `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION`.

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
            disable_timer_resolution_throttling();
        }
        Self { _private: () }
    }
}

/// 本プロセスの timer resolution に対する Power Throttling を無効化する
///
/// `SetProcessInformation` + `ProcessPowerThrottling` で
/// `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` を立て、本プロセスが
/// バックグラウンドに回っても OS が要求 timer resolution を自動的に引き下げない
/// よう要求する。Windows 以外では何もしない no-op。失敗時は `tracing::warn!`
/// で続行する (fatal にはしない: 多少 tempo が揺れても再生自体は継続させたい)。
///
/// Opts this process out of timer resolution Power Throttling on Windows
/// 10 2004 and later. No-op on non-Windows. Failures are logged via
/// `tracing::warn!` and execution continues — the audible tempo may drift
/// when the process is backgrounded, but playback should still run.
#[cfg(windows)]
fn disable_timer_resolution_throttling() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE,
    };

    // ControlMask で IGNORE_TIMER_RESOLUTION を「設定対象」に指定し、
    // StateMask で同ビットを 1 にすることで「自動引き下げを無効化」する。
    // ControlMask selects which throttling features are being configured;
    // StateMask gives the desired state for those features.
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    };

    // SAFETY: GetCurrentProcess は擬似ハンドルを返す純粋な API で常に成功する。
    // SetProcessInformation には正しい info class / 構造体 / サイズを渡しており、
    // state は本関数のスタック上にあり呼び出し中生存している。
    // SAFETY: GetCurrentProcess returns a pseudo-handle. SetProcessInformation
    // is called with the matching info class / struct / size, and `state`
    // lives on the stack for the duration of the call.
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            core::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };

    if ok != 0 {
        tracing::info!(
            "Windows Power Throttling (timer resolution) を本プロセスについて無効化しました"
        );
    } else {
        // GetLastError は windows-sys では Win32_Foundation 配下。ここでは値を
        // 取得して warn に含めるだけで処理は続行する。
        // GetLastError lives under Win32_Foundation. We just surface it in the
        // log; the call site continues regardless.
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        tracing::warn!(
            "SetProcessInformation(ProcessPowerThrottling) が失敗しました (GetLastError={}). \
             バックグラウンド時に tempo が遅延する可能性があります",
            err
        );
    }
}

/// 非 Windows 向けの no-op 実装
/// No-op stub for non-Windows targets.
#[cfg(not(windows))]
#[allow(dead_code)]
fn disable_timer_resolution_throttling() {}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// `acquire()` が panic せずにガードを返し、Drop まで走る事を確認する。
    /// Windows 上では実 API 呼び出し、Linux 等では no-op。どちらでもパスする。
    ///
    /// Smoke test that `acquire()` returns a guard without panicking and
    /// that Drop runs cleanly. Real API calls happen on Windows; elsewhere
    /// the call chain is a no-op.
    #[test]
    fn acquire_returns_guard_and_drops_cleanly() {
        let guard = HighResolutionTimer::acquire();
        drop(guard);
    }

    /// `disable_timer_resolution_throttling()` が単独で呼ばれても panic しない事を
    /// 確認する。Windows では実 API を叩くが、失敗時も warn ログのみで return する
    /// 設計のため panic はしない。非 Windows では no-op。
    ///
    /// Verifies that `disable_timer_resolution_throttling()` does not panic
    /// when called directly. On Windows it invokes the real API but surfaces
    /// failures only via `tracing::warn!`; on other platforms it is a no-op.
    #[test]
    fn disable_throttling_does_not_panic() {
        disable_timer_resolution_throttling();
    }
}
