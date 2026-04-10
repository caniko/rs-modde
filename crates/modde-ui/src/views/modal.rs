//! Reusable modal overlay widget.
//!
//! Renders a centered confirmation card over a dimmed full-screen
//! backdrop. Composed into the app's top-level `view()` via
//! `iced::widget::stack![base, backdrop, card]` so the modal sits above
//! the normal UI and captures hit-testing before falling through.
//!
//! There is exactly one modal active at a time — `Modde` stores the
//! state as `Option<ModalState>`. When the state is `None`, [`overlay`]
//! returns `None` and the caller can skip the `stack!` wrap entirely
//! (the no-modal path adds zero widgets to the render tree).
//!
//! **Dismissal:** clicking the backdrop dispatches
//! [`Message::DismissModal`]. Clicks on the card itself are absorbed by
//! an inner `mouse_area` so they don't fall through to the backdrop.
//! Pressing **Escape** also dispatches `Message::DismissModal` via the
//! app-level keyboard subscription (see `Modde::subscription` in
//! [`crate::app`]), routed through [`crate::shortcuts::match_shortcut`].

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{
    color, Alignment, Background, Border, Color, Element, Length, Shadow, Theme, Vector,
};

use crate::app::{Message, ModalState};

// The `lock_description` we render is pre-formatted by
// `format_lock_reason` in `app.rs` at the time the modal is opened
// (see the `Message::ShowUnlockConfirm` handler). The modal itself
// receives a plain `&str` so it doesn't depend on the LockReason enum.

/// Render the current modal as a `(backdrop, card)` pair suitable for
/// passing to `iced::widget::stack![base, backdrop, card]`.
///
/// Returns `None` when no modal is open — callers should skip the
/// `stack!` wrap entirely in that case to avoid allocating unnecessary
/// widgets on every render pass.
pub fn overlay<'a>(state: Option<&'a ModalState>) -> Option<[Element<'a, Message>; 2]> {
    let state = state?;
    Some(match state {
        ModalState::ConfirmUnlockLoadOrder { lock_description } => {
            [backdrop(), confirm_unlock_card(lock_description)]
        }
    })
}

/// Full-screen dimmed backdrop. Clicking anywhere on it dismisses the
/// modal. Styled as semi-transparent black (~65% opacity) so the
/// underlying UI is still faintly visible — enough contrast to pull
/// focus onto the card without being a hard lockout.
fn backdrop<'a>() -> Element<'a, Message> {
    mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.65))),
                ..Default::default()
            }),
    )
    .on_press(Message::DismissModal)
    .into()
}

/// Centered confirmation card for the unlock-load-order dialog.
///
/// `lock_description` is the pre-formatted reason string captured when
/// the modal was opened (see `Message::ShowUnlockConfirm` handler in
/// `app.rs`). The handler snapshots it so the dialog text stays stable
/// even if the profile changes underneath.
fn confirm_unlock_card<'a>(lock_description: &'a str) -> Element<'a, Message> {
    let title = text("Release load order lock?")
        .size(18)
        .color(color!(0xFFAA44));

    let body = text(format!(
        "This profile is locked by {lock_description}. Unlocking will \
         allow mods to be reordered, which may break the installed \
         modlist's intended behaviour."
    ))
    .size(13);

    let warning = text(
        "You can re-lock the profile manually at any time via the Lock \
         button, but the original provenance metadata (Wabbajack hash, \
         Collection version, etc.) will not be restored automatically.",
    )
    .size(12)
    .color(color!(0xAAAAAA));

    // Button row is right-aligned via a leading Fill-spacer so Cancel
    // and Unlock hug the card's right edge, matching the convention of
    // most native OS confirmation dialogs. `button::danger` gives
    // Unlock more visual weight than the secondary Cancel.
    let buttons = row![
        Space::new().width(Length::Fill),
        button(text("Cancel").size(14))
            .on_press(Message::DismissModal)
            .style(button::secondary)
            .padding([6, 16]),
        button(text("Unlock").size(14))
            .on_press(Message::UnlockLoadOrder)
            .style(button::danger)
            .padding([6, 16]),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // The dialog card itself. `max_width` stops it from stretching to
    // absurd widths on ultrawide monitors; `padding` gives breathing
    // room inside the frame.
    let card_content = column![title, body, warning, buttons].spacing(12);

    // Custom card style: theme-aware panel background (slightly lighter
    // than the base surface so it lifts off the dimmed backdrop), 1px
    // neutral border, 10px rounded corners, and a soft drop shadow.
    // Using a closure instead of `container::bordered_box` so we can
    // add the shadow and pick a rounder radius.
    let styled_card = container(card_content)
        .padding(20)
        .max_width(480)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                text_color: Some(palette.background.weak.text),
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 10.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 16.0,
                },
                ..Default::default()
            }
        });

    // Absorb clicks on the card so they don't fall through to the
    // backdrop and dismiss the dialog. `Message::Noop` is a no-op
    // already defined on the Message enum.
    let card = mouse_area(styled_card).on_press(Message::Noop);

    // Center the card over the backdrop. Using `Length::Fill` on both
    // axes with `center_x` / `center_y` is the iced 0.14 idiom for
    // "place this in the middle of the available space".
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
