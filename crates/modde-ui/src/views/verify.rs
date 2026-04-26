use crate::views::selectable_text::text;
use iced::widget::{button, column, container, row, scrollable};
use iced::{Alignment, Element, Length, color};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, VerifyState};

/// Render the verification results view.
pub fn view(state: &VerifyState) -> Element<'_, Message> {
    let running = matches!(state, VerifyState::Running);
    let title_bar = row![
        text("Verification").size(20),
        iced::widget::space::horizontal(),
        button(text(if running { "Running..." } else { "Run Verify" }).size(14))
            .style(button::primary)
            .padding([6, 14])
            .on_action_maybe(
                (!running).then_some(ButtonAction::RunVerify),
                "Verification is already running.",
            ),
    ]
    .align_y(Alignment::Center);

    let content: Element<Message> = match state {
        VerifyState::Idle | VerifyState::Running => {
            container(text("Click 'Run Verify' to check installed mod integrity.").size(14))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
        }

        VerifyState::Complete(results) => {
            let mut col = column![].spacing(12);

            // Summary
            let ok_text = text(format!("{} file(s) OK", results.ok_count))
                .size(16)
                .color(color!(0x88CC88));
            col = col.push(ok_text);

            // Broken symlinks
            if !results.broken_symlinks.is_empty() {
                let broken_header = text(format!(
                    "{} broken symlink(s)",
                    results.broken_symlinks.len()
                ))
                .size(16)
                .color(color!(0xFF4444));

                let broken_list = results
                    .broken_symlinks
                    .iter()
                    .fold(column![].spacing(2), |col, path| {
                        col.push(text(path.display().to_string()).size(12))
                    });

                col = col.push(broken_header);
                col = col.push(
                    container(broken_list)
                        .padding(8)
                        .width(Length::Fill)
                        .style(container::rounded_box),
                );
            }

            // Hash mismatches
            if !results.hash_mismatches.is_empty() {
                let mismatch_header = text(format!(
                    "{} hash mismatch(es)",
                    results.hash_mismatches.len()
                ))
                .size(16)
                .color(color!(0xFF8844));

                let mismatch_list = results.hash_mismatches.iter().fold(
                    column![].spacing(2),
                    |col, (path, expected, actual)| {
                        col.push(
                            text(format!(
                                "{}: expected {}, got {}",
                                path.display(),
                                expected,
                                actual
                            ))
                            .size(12),
                        )
                    },
                );

                col = col.push(mismatch_header);
                col = col.push(
                    container(mismatch_list)
                        .padding(8)
                        .width(Length::Fill)
                        .style(container::rounded_box),
                );
            }

            // Missing mods
            if !results.missing_mods.is_empty() {
                let missing_header = text(format!("{} missing mod(s)", results.missing_mods.len()))
                    .size(16)
                    .color(color!(0xFF8844));

                let missing_list = results
                    .missing_mods
                    .iter()
                    .fold(column![].spacing(2), |col, mod_id| {
                        col.push(text(mod_id).size(12))
                    });

                col = col.push(missing_header);
                col = col.push(
                    container(missing_list)
                        .padding(8)
                        .width(Length::Fill)
                        .style(container::rounded_box),
                );
            }

            if results.broken_symlinks.is_empty()
                && results.hash_mismatches.is_empty()
                && results.missing_mods.is_empty()
            {
                col = col.push(
                    text("All files verified successfully!")
                        .size(14)
                        .color(color!(0x88CC88)),
                );
            }

            scrollable(col.padding(8)).height(Length::Fill).into()
        }
    };

    column![title_bar, iced::widget::rule::horizontal(1), content]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
