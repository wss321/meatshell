// Process-global core shared by every window. Slint is single-threaded:
// all windows live on the same UI thread, so the Rc<RefCell<>> members are
// only ever touched from that thread (the listener registry is written once
// per window construction, also on the UI thread).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use tokio::runtime::Runtime;

use crate::config::ConfigStore;
use crate::ui::AppWindow;

/// Open-window registry, generic over the window handle so it can be unit
/// tested without constructing Slint components. Production instantiates
/// `WindowRegistry<slint::Weak<AppWindow>>`.
#[derive(Default)]
pub struct WindowRegistry<H> {
    next_id: RefCell<u64>,
    windows: RefCell<HashMap<u64, H>>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl<H: Clone> WindowRegistry<H> {
    pub fn register(&self, handle: H) -> u64 {
        let mut next = self.next_id.borrow_mut();
        *next += 1;
        let id = *next;
        self.windows.borrow_mut().insert(id, handle);
        id
    }

    /// Remove a window; returns true when the registry became empty (the
    /// caller must then quit the event loop).
    pub fn unregister(&self, id: u64) -> bool {
        self.windows.borrow_mut().remove(&id);
        self.windows.borrow().is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.borrow().is_empty()
    }

    pub fn count(&self) -> usize {
        self.windows.borrow().len()
    }

    pub fn for_each<F: FnMut(&H)>(&self, mut f: F) {
        for h in self.windows.borrow().values() {
            f(h);
        }
    }

    /// A handle of the most recently registered window (used as cascade /
    /// position origin for the next one).
    pub fn newest(&self) -> Option<H> {
        self.windows
            .borrow()
            .iter()
            .max_by_key(|(id, _)| *id)
            .map(|(_, h)| h.clone())
    }

    pub fn add_config_listener(&self, f: Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(f);
    }

    /// Config (sessions / theme / language …) changed in one window; every
    /// window refreshes its derived UI state.
    pub fn broadcast_config_changed(&self) {
        for l in self.listeners.borrow().iter() {
            l();
        }
    }
}

pub struct AppCore {
    pub runtime: Arc<Runtime>,
    /// Shared among all windows; touched only on the Slint UI thread.
    pub store: Rc<RefCell<ConfigStore>>,
    /// Live windows; the last one closing quits the shared event loop.
    pub registry: Rc<WindowRegistry<slint::Weak<AppWindow>>>,
}

#[cfg(test)]
#[path = "../../tests/app/window_management/registry.rs"]
mod registry_tests;
