use iced::widget::{button, column, container, row, scrollable, text};
use iced::{color, Alignment, Element, Length};

use modde_core::save::{SaveFingerprint, SaveSnapshot};

use crate::app::Message;

/// Render the save management view.
pub fn view<'a>(
    snapshots: &'a [SaveSnapshot],
    profile_name: Option<&'a str>,
    current_fingerprint: Option<&'a SaveFingerprint>,
) -> Element<'a, Message> {
    let title_bar = row![
        text("Save Management").size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(14))
            .on_press(Message::LoadSaveHistory)
            .style(button::secondary)
            .padding([6, 14]),
    ]
    .align_y(Alignment::Center);

    let mut profile_info = column![].spacing(2);
    profile_info = profile_info.push(match profile_name {
        Some(name) => text(format!("Profile: {name}")).size(14),
        None => text("No active profile").size(14).color(color!(0xFF8844)),
    });

    // Show current fingerprint
    if let Some(fp) = current_fingerprint {
        if !fp.is_empty() {
            profile_info = profile_info.push(
                text(format!(
                    "Save-breaking mods: {} [{}]",
                    fp.mod_ids.len(),
                    fp.short_hash()
                ))
                .size(12)
                .color(color!(0x888888)),
            );
        }
    }

    let header = row![
        text("ID").size(12).width(Length::Fixed(80.0)),
        text("Message").size(12).width(Length::Fill),
        text("Files").size(12).width(Length::Fixed(50.0)),
        text("Mods").size(12).width(Length::Fixed(60.0)),
        text("").size(12).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .padding([4, 0]);

    let content: Element<Message> = if snapshots.is_empty() {
        container(
            text("No save snapshots found. Saves are captured automatically when switching profiles.")
                .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let rows = snapshots.iter().fold(column![].spacing(2), |col, snap| {
            let short_id = text(snap.short_id()).size(12).width(Length::Fixed(80.0));
            let msg_line = snap.message.lines().next().unwrap_or("").trim();
            let message = text(msg_line).size(13).width(Length::Fill);
            let file_count = text(format!("{}", snap.file_count)).size(12).width(Length::Fixed(50.0));

            // Fingerprint indicator
            let fp_indicator: Element<Message> = match &snap.fingerprint {
                Some(fp) if current_fingerprint.is_some() => {
                    let check = snap.check_compatibility(current_fingerprint.unwrap());
                    match check {
                        modde_core::save::FingerprintCheck::Compatible => {
                            text(fp.short_hash()).size(11).color(color!(0x44AA44)).width(Length::Fixed(60.0)).into()
                        }
                        modde_core::save::FingerprintCheck::Mismatch { ref removed, ref added } => {
                            let delta = format!("-{}/+{}", removed.len(), added.len());
                            text(delta).size(11).color(color!(0xFF6644)).width(Length::Fixed(60.0)).into()
                        }
                        modde_core::save::FingerprintCheck::NoFingerprint => {
                            text("—").size(11).color(color!(0x666666)).width(Length::Fixed(60.0)).into()
                        }
                    }
                }
                Some(fp) => {
                    text(fp.short_hash()).size(11).color(color!(0x888888)).width(Length::Fixed(60.0)).into()
                }
                None => {
                    text("—").size(11).color(color!(0x666666)).width(Length::Fixed(60.0)).into()
                }
            };

            let commit_id = snap.id.clone();
            let restore_btn = button(text("Restore").size(12))
                .on_press(Message::RestoreSaveSnapshot(commit_id))
                .style(button::secondary)
                .padding([4, 8]);

            let snapshot_row = row![
                short_id,
                message,
                file_count,
                fp_indicator,
                restore_btn,
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([4, 8]);

            col.push(snapshot_row)
        });

        scrollable(rows).height(Length::Fill).into()
    };

    let status = text(format!("{} snapshot(s)", snapshots.len())).size(12);

    column![
        title_bar,
        profile_info,
        header,
        iced::widget::rule::horizontal(1),
        content,
        status,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
