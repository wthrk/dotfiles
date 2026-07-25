//! YubiKey PIV backend の process-local technical observation hook。
//!
//! support backend は固定 operation と開始/成功/opaque failure だけを通知する。CLI phase への
//! 変換と command enablement は YubiKey presentation/composition が所有する。

use std::cell::Cell;

#[derive(Clone, Copy)]
pub(crate) enum Observation {
    Started,
    Succeeded,
    Failed,
}
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    SessionOpen,
    VerifyInvocation,
    ManagementKeyAuthentication,
}
type Observer = fn(Operation, Observation);
thread_local! { static OBSERVER: Cell<Option<Observer>> = const { Cell::new(None) }; }
pub(crate) struct Scope {
    previous: Option<Observer>,
}
impl Scope {
    pub(crate) fn new(observer: Observer) -> Self {
        let previous = OBSERVER.with(|current| {
            let previous = current.get();
            current.set(Some(observer));
            previous
        });
        Self { previous }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        OBSERVER.with(|current| current.set(self.previous));
    }
}
pub(crate) fn started(operation: Operation) {
    notify(operation, Observation::Started);
}
pub(crate) fn returned<T>(operation: Operation, result: &crate::Result<T>) {
    notify(
        operation,
        if result.is_ok() {
            Observation::Succeeded
        } else {
            Observation::Failed
        },
    );
}
fn notify(operation: Operation, observation: Observation) {
    OBSERVER.with(|observer| {
        if let Some(observer) = observer.get() {
            observer(operation, observation);
        }
    });
}
