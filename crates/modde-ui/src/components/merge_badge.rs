use iced::widget::container;
use iced::{Element, Length, color};
use modde_core::merge::{MergeSession, MergeStatus};

use crate::app::Message;
use crate::views::selectable_text::text;

pub fn render_status(session: Option<&MergeSession>) -> Element<'static, Message> {
    let Some(session) = session else {
        return iced::widget::Space::new().into();
    };
    let Some(label) = status_label(Some(session)) else {
        return iced::widget::Space::new().into();
    };
    let badge_color = match session.status {
        MergeStatus::Pending => color!(0xFFD166),
        MergeStatus::Resolved => color!(0x6BD968),
        MergeStatus::Conflicted => color!(0xFF6666),
        MergeStatus::Stale => color!(0xFF9F43),
    };

    container(text(label).size(10).color(badge_color))
        .width(Length::Shrink)
        .padding([1, 4])
        .style(container::rounded_box)
        .into()
}

#[must_use]
pub fn status_label(session: Option<&MergeSession>) -> Option<&'static str> {
    match session.map(|session| session.status) {
        None => None,
        Some(MergeStatus::Pending) => Some("Needs merge"),
        Some(MergeStatus::Resolved) => Some("Merged"),
        Some(MergeStatus::Conflicted) => Some("Conflict - re-run"),
        Some(MergeStatus::Stale) => Some("Stale - re-run"),
    }
}
