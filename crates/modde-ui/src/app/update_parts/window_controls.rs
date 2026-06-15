#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! window_controls update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_window_controls_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Window controls (custom title bar) ───────────────
            Message::GotWindowId(Some(id)) => {
                self.window_id = id;
            }
            Message::GotWindowId(None) => {}
            Message::TitleBarDrag => {
                return window::drag(self.window_id);
            }
            Message::WindowMinimize => {
                return window::minimize(self.window_id, true);
            }
            Message::WindowToggleMaximize => {
                return window::toggle_maximize(self.window_id);
            }
            Message::WindowClose => {
                return window::close(self.window_id);
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
