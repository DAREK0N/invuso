//! App-wide state shared through the Dioxus context (idee.md 2.3 `state/`).
//! Both are provided above the router, so they outlive single screens.

use std::rc::Rc;

use dioxus::prelude::*;

/// Counter bumped after every write to the database. Screens read it inside
/// their queries (`use_memo`) and reload when data changed elsewhere, e.g.
/// through the undo button of a toast.
#[derive(Clone, Copy, PartialEq)]
pub struct DataRevision(Signal<u64>);

impl DataRevision {
    /// Must be called inside a component, like any `Signal::new`.
    pub fn new() -> Self {
        Self(Signal::new(0))
    }

    /// Subscribes the calling memo or component to data changes.
    pub fn track(&self) {
        self.0.read();
    }

    /// Marks the data as changed.
    pub fn bump(&mut self) {
        *self.0.write() += 1;
    }
}

impl Default for DataRevision {
    fn default() -> Self {
        Self::new()
    }
}

/// Button of a toast, e.g. "Undo".
#[derive(Clone)]
pub struct ToastAction {
    pub label: String,
    /// Owned by the toast, not by the screen that showed it: the screen may
    /// be gone (e.g. after navigating back) when the button is tapped.
    pub run: Rc<dyn Fn()>,
}

/// Short message at the bottom of the screen (UI-11).
#[derive(Clone)]
pub struct Toast {
    /// Distinguishes consecutive toasts, so a timer only hides its own.
    pub id: u64,
    pub message: String,
    pub action: Option<ToastAction>,
}

/// Shows toasts; the one current toast is rendered by `ToastHost`.
#[derive(Clone, Copy, PartialEq)]
pub struct Toaster {
    current: Signal<Option<Toast>>,
    next_id: Signal<u64>,
}

impl Toaster {
    /// Must be called inside a component, like any `Signal::new`.
    pub fn new() -> Self {
        Self {
            current: Signal::new(None),
            next_id: Signal::new(0),
        }
    }

    /// Replaces any visible toast.
    pub fn show(&mut self, message: String, action: Option<ToastAction>) {
        let id = *self.next_id.peek();
        self.next_id.set(id + 1);
        self.current.set(Some(Toast {
            id,
            message,
            action,
        }));
    }

    pub fn current(&self) -> Option<Toast> {
        self.current.read().clone()
    }

    /// Hides the toast with this id, if it is still the visible one.
    pub fn dismiss(&mut self, id: u64) {
        let visible = self.current.peek().as_ref().map(|toast| toast.id);
        if visible == Some(id) {
            self.current.set(None);
        }
    }
}

impl Default for Toaster {
    fn default() -> Self {
        Self::new()
    }
}
