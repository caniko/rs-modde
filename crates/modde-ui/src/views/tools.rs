use crate::views::selectable_text::text;
use iced::widget::{button, column, container, row, scrollable, toggler};
use iced::{Alignment, Element, Length, color};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, ToolState, ToolUiEntry};

/// Render the gaming tools/overlays management view.
pub fn view(state: &ToolState) -> Element<'_, Message> {
    let title_bar = row![
        text("Gaming Tools").size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(14))
            .style(button::secondary)
            .padding([6, 14])
            .on_action(ButtonAction::RefreshTools),
    ]
    .align_y(Alignment::Center);

    let content: Element<'_, Message> = if state.entries.is_empty() {
        container(text("Select a game to manage tools, or click Refresh.").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
    } else {
        let cards = state.entries.iter().fold(
            column![].spacing(8),
            |col: iced::widget::Column<'_, Message>, entry| col.push(tool_card(entry)),
        );

        scrollable(container(cards).padding(12).width(Length::Fill)).into()
    };

    column![title_bar, content]
        .spacing(12)
        .padding(12)
        .width(Length::Fill)
        .into()
}

/// Render a single tool card.
fn tool_card(entry: &ToolUiEntry) -> Element<'_, Message> {
    let tool_id = entry.tool_id.clone();
    let available = entry.available;

    // Header row: name + category + toggle
    let name_text = text(entry.display_name.as_str()).size(16);
    let category_text = text(entry.category.as_str())
        .size(11)
        .color(color!(0x888888));

    let avail_text = if available {
        text("installed").size(11).color(color!(0x88CC88))
    } else {
        text("not installed").size(11).color(color!(0xFF6666))
    };

    let toggle = toggler(entry.enabled)
        .on_toggle({
            let tid = tool_id.clone();
            move |enabled| Message::ToggleTool {
                tool_id: tid.clone(),
                enabled,
            }
        })
        .size(18.0);

    let header = row![
        column![name_text, category_text].spacing(2),
        iced::widget::space::horizontal(),
        avail_text,
        toggle,
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    // Applied files count
    let mut body = column![].spacing(4);

    if entry.applied_files > 0 {
        body = body.push(
            text(format!(
                "{} file(s) applied to game directory",
                entry.applied_files
            ))
            .size(12)
            .color(color!(0xAAAA66)),
        );
    }

    // Action buttons (for patching tools)
    if entry.has_file_patching {
        let tid_apply = tool_id.clone();
        let tid_revert = tool_id.clone();

        let actions = row![
            button(text("Apply").size(12))
                .style(button::primary)
                .padding([4, 10])
                .on_action_maybe(
                    available.then_some(ButtonAction::ApplyTool(tid_apply)),
                    "Install or enable this tool before applying its files.",
                ),
            button(text("Revert").size(12))
                .style(button::danger)
                .padding([4, 10])
                .on_action_maybe(
                    (entry.applied_files > 0).then_some(ButtonAction::RevertTool(tid_revert)),
                    "This tool has no applied files to revert.",
                ),
        ]
        .spacing(6);

        body = body.push(actions);
    }

    // Status message
    if let Some(ref msg) = entry.status_message {
        body = body.push(text(msg.as_str()).size(12).color(color!(0x88CC88)));
    }

    let card = column![header, body].spacing(6);

    container(card)
        .padding(10)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}
