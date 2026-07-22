//! PIV management session の opt-in・非秘匿 diagnostic を出力する technical helper。
//!
//! scope 内だけが fixed phase を stderr に出し、PIN、token、SDK error、APDU/status を受け取らない。
//! PIV backend は実際の open / VERIFY / authentication 呼び出しの直前と復帰時だけを通知するため、
//! diagnostic のために device operation、retry、mutation を追加しない。

use std::cell::Cell;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// 現在 thread の PIV technical diagnostic を command 実行中だけ有効にする guard。
pub(crate) struct PivDebugScope {
    previous: bool,
}

impl PivDebugScope {
    /// opt-in diagnostic の有効状態を保存して設定する。
    pub(crate) fn new(enabled: bool) -> Self {
        let previous = ENABLED.with(|current| {
            let previous = current.get();
            current.set(enabled);
            previous
        });
        Self { previous }
    }
}

impl Drop for PivDebugScope {
    fn drop(&mut self) {
        ENABLED.with(|current| current.set(self.previous));
    }
}

/// technical phase の開始だけを fixed schema で出力する。
pub(crate) fn started(phase: &str) {
    ENABLED.with(|enabled| {
        if enabled.get() {
            eprintln!("dotfiles-piv-debug phase={phase}-started");
        }
    });
}

/// technical operation の復帰を、成功または opaque failure としてだけ出力する。
pub(crate) fn returned<T>(phase: &str, result: &crate::Result<T>) {
    ENABLED.with(|enabled| {
        if enabled.get() {
            let suffix = if result.is_ok() {
                "succeeded"
            } else {
                "returned result=opaque-error"
            };
            eprintln!("dotfiles-piv-debug phase={phase}-{suffix}");
        }
    });
}
