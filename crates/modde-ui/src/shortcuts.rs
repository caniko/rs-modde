use iced::keyboard;

use crate::app::Message;

/// Map a keyboard event to an app message, if it matches a shortcut.
pub fn handle_key_event(
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Option<Message> {
    match (modifiers, &key) {
        // F5 = refresh / reload profile
        (m, keyboard::Key::Named(keyboard::key::Named::F5)) if m.is_empty() => {
            Some(Message::Noop) // TODO: wire to actual refresh
        }
        // Escape = cancel current overlay (e.g. FOMOD wizard)
        (m, keyboard::Key::Named(keyboard::key::Named::Escape)) if m.is_empty() => {
            Some(Message::FOMODCancel)
        }
        _ => None,
    }
}
