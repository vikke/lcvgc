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
            // 診断ログ: Windows ビルドと現在の実 timer resolution。
            // PR #59 までの fix で API が「成功」と返っても tempo が遅い事例が出ているため、
            // 「実際に何 ns 粒度で動いているか」「ビルド番号は IGNORE_TIMER_RESOLUTION の
            // サポート対象 (22H2 / build 22621 以降) か」を可視化する。
            log_windows_build_info();
            log_current_timer_resolution();
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

/// 現在の実 timer resolution を `NtQueryTimerResolution` で取得し info ログに出す
///
/// `timeBeginPeriod(1)` や Power Throttling 抑止が「成功」を返しても、Windows の
/// 実 timer resolution は他プロセスとの集約や OS 側の制約で別の値になっている事が
/// ある。本関数は「現在 / 最小 / 最大」の 3 値 (100ns 単位) をそのまま info で出し、
/// ユーザー側で「本当に 1ms 粒度になっているか」を確認できるようにする。
/// 現在値が 10000 (= 1ms) を超えていたら warn を併発する。
///
/// Logs the actual current / minimum / maximum timer resolution returned by
/// `NtQueryTimerResolution`. Values are in 100ns units. Warns if `current` is
/// worse than 1ms (i.e. `current > 10_000`).
#[cfg(windows)]
fn log_current_timer_resolution() {
    use windows_sys::Wdk::System::SystemInformation::NtQueryTimerResolution;

    let mut maximum: u32 = 0;
    let mut minimum: u32 = 0;
    let mut current: u32 = 0;
    // SAFETY: NtQueryTimerResolution は 3 つの out 引数に書き戻すだけの API で、
    // それぞれローカル u32 のスタックに格納される。NTSTATUS が非ゼロの場合は
    // out 引数の中身は未定義扱いとし、ログ出力もスキップする。
    // SAFETY: NtQueryTimerResolution writes into three u32 out-params. On non-
    // zero NTSTATUS we treat the out-params as undefined and skip the log.
    let status = unsafe { NtQueryTimerResolution(&mut maximum, &mut minimum, &mut current) };
    if status != 0 {
        tracing::warn!(
            "NtQueryTimerResolution が失敗しました (NTSTATUS=0x{:08X})",
            status as u32
        );
        return;
    }
    // NB: ntdll の語法では maximum = 一番粗い値, minimum = 一番細かい値。
    // 数値としては maximum >= current >= minimum となる。表示はそのまま 100ns 単位。
    tracing::info!(
        "現在 timer resolution: current={} ×100ns ({} us), min={} ×100ns, max={} ×100ns",
        current,
        current / 10,
        minimum,
        maximum
    );
    if current > 10_000 {
        tracing::warn!(
            "実 timer resolution が 1ms より粗い ({} us). \
             timeBeginPeriod(1) と Power Throttling 抑止が効いていない可能性があります",
            current / 10
        );
    }
}

/// 非 Windows 向け no-op
#[cfg(not(windows))]
#[allow(dead_code)]
fn log_current_timer_resolution() {}

/// `RtlGetVersion` で Windows ビルド情報を取得し info ログに出す
///
/// `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` は Windows 11 22H2
/// (build 22621) 以降でしか機能しないため、それ未満の場合は warn を併発する。
/// `GetVersionExW` は manifest に依存して古い値を返す事があるが、`RtlGetVersion`
/// は manifest を介さず実 OS バージョンを返すためこちらを使う。
///
/// Logs the Windows build via `RtlGetVersion` (manifest-independent). Warns
/// when build number is below 22621 (Windows 11 22H2), where
/// `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` is not supported.
#[cfg(windows)]
fn log_windows_build_info() {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    // OSVERSIONINFOW は dwOSVersionInfoSize に自身のサイズをセットして渡す API。
    // それ以外は RtlGetVersion 側で埋めてくれるため zeroed で初期化し size のみ書く。
    // OSVERSIONINFOW requires the caller to set dwOSVersionInfoSize; the rest
    // is filled in by RtlGetVersion.
    let mut info: OSVERSIONINFOW = unsafe { core::mem::zeroed() };
    info.dwOSVersionInfoSize = core::mem::size_of::<OSVERSIONINFOW>() as u32;
    // SAFETY: 上で OSVERSIONINFOW をゼロ初期化しサイズフィールドを設定済み。
    // RtlGetVersion は同構造体を書き込むだけで out 範囲は info 自身に収まる。
    // SAFETY: `info` was zero-initialized above with the correct size set.
    let status = unsafe { RtlGetVersion(&mut info) };
    if status != 0 {
        tracing::warn!(
            "RtlGetVersion が失敗しました (NTSTATUS=0x{:08X})",
            status as u32
        );
        return;
    }
    tracing::info!(
        "Windows ビルド: {}.{} build={}",
        info.dwMajorVersion,
        info.dwMinorVersion,
        info.dwBuildNumber
    );
    // 22H2 = build 22621。それ未満では IGNORE_TIMER_RESOLUTION フラグは
    // SetProcessInformation が「成功」を返しても OS 側で無視される。
    if info.dwBuildNumber < 22621 {
        tracing::warn!(
            "Windows build {} は PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION 非対応 \
             (要 build 22621 / Windows 11 22H2 以降). \
             バックグラウンド時 tempo が遅延する可能性があります",
            info.dwBuildNumber
        );
    }
}

/// 非 Windows 向け no-op
#[cfg(not(windows))]
#[allow(dead_code)]
fn log_windows_build_info() {}

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

    /// `log_current_timer_resolution()` が panic しない事を確認する smoke test。
    /// Windows では NtQueryTimerResolution を呼び、Linux 等では no-op。失敗時も
    /// warn ログだけで return する設計のため panic しない。
    ///
    /// Smoke test: `log_current_timer_resolution()` must not panic on any
    /// platform. Windows hits ntdll, others are no-ops.
    #[test]
    fn log_current_timer_resolution_does_not_panic() {
        log_current_timer_resolution();
    }

    /// `log_windows_build_info()` が panic しない事を確認する smoke test。
    /// Windows では RtlGetVersion を呼び、それ以外では no-op。
    ///
    /// Smoke test: `log_windows_build_info()` must not panic on any
    /// platform.
    #[test]
    fn log_windows_build_info_does_not_panic() {
        log_windows_build_info();
    }
}
