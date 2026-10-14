//! When the browser starts drawing each frame: the timestamps it passes to
//! `requestAnimationFrame` callbacks, which follow the screen's refreshes to
//! the microsecond. A UI toolkit's frame ticks, such as Iced's, only reach
//! the app some milliseconds later, by an amount that varies with how long
//! drawing took, which is too uneven to count refreshes with at 120 Hz.
//!
//! The clock asks for the next frame from inside its own callback, so once
//! its callback runs first in a frame it stays first: it starts while the
//! game starts, before the app asks to draw the next frame, and so has
//! already recorded a frame's time when that frame's tick reaches the
//! session.

use std::cell::Cell;
use std::rc::Rc;
use web_sys::wasm_bindgen::JsCast;
use web_sys::wasm_bindgen::closure::Closure;

/// Records when the browser starts drawing each frame, until it is dropped.
pub struct RefreshClock {
    state: Rc<State>,
}

/// What the clock's callbacks share with it.
#[derive(Default)]
struct State {
    /// When the browser started drawing the latest frame, in milliseconds
    /// since the page loaded, or `None` before the first.
    last_refresh: Cell<Option<f64>>,
    /// Whether the clock was dropped, which ends the callbacks.
    stopped: Cell<bool>,
}

impl RefreshClock {
    /// Starts recording from the next frame on.
    pub fn start() -> Self {
        let state = Rc::new(State::default());
        record_next_frame(state.clone());
        RefreshClock { state }
    }

    /// When the browser started drawing the latest frame, in seconds since
    /// the page loaded, or `None` before the first.
    pub fn last_refresh(&self) -> Option<f64> {
        self.state.last_refresh.get().map(|millis| millis / 1000.0)
    }
}

impl Drop for RefreshClock {
    fn drop(&mut self) {
        self.state.stopped.set(true);
    }
}

/// Asks the browser to record the time of the next frame in `state`, and to
/// ask again from there, until the clock is dropped.
fn record_next_frame(state: Rc<State>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let callback = Closure::once_into_js(move |timestamp: f64| {
        if state.stopped.get() {
            return;
        }
        state.last_refresh.set(Some(timestamp));
        record_next_frame(state);
    });
    let _ = window.request_animation_frame(callback.unchecked_ref());
}
