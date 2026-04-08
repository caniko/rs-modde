use iced::widget::{button, column, container, row, scrollable, text};
use iced::{color, Alignment, Element, Length};

use crate::app::Message;

/// A single diagnostic finding.
#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

/// Severity levels for diagnostics.
#[derive(Debug, Clone)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// State machine for the diagnostics view.
#[derive(Debug, Clone, Default)]
pub enum DiagnosticsState {
    #[default]
    Idle,
    Running,
    Complete(Vec<DiagnosticEntry>),
}

/// Render the diagnostics view.
pub fn view(state: &DiagnosticsState) -> Element<'_, Message> {
    let running = matches!(state, DiagnosticsState::Running);

    let title_bar = row![
        text("Diagnostics").size(20),
        iced::widget::space::horizontal(),
        button(
            text(if running {
                "Running..."
            } else {
                "Run Diagnostics"
            })
            .size(14)
        )
        .on_press_maybe(if running {
            None
        } else {
            Some(Message::RunDiagnostics)
        })
        .style(button::primary)
        .padding([6, 14]),
    ]
    .align_y(Alignment::Center);

    let content: Element<Message> = match state {
        DiagnosticsState::Idle | DiagnosticsState::Running => container(
            text("Click 'Run Diagnostics' to scan for common modding issues.").size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into(),

        DiagnosticsState::Complete(entries) => {
            if entries.is_empty() {
                container(
                    text("No issues found!")
                        .size(14)
                        .color(color!(0x88CC88)),
                )
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
            } else {
                let rows =
                    entries
                        .iter()
                        .fold(column![].spacing(4), |col, entry| {
                            let (icon, icon_color) = match entry.severity {
                                DiagnosticSeverity::Info => ("INFO", color!(0x88AACC)),
                                DiagnosticSeverity::Warning => ("WARN", color!(0xFFAA44)),
                                DiagnosticSeverity::Error => ("ERR ", color!(0xFF4444)),
                            };

                            let entry_row = row![
                                text(icon).size(12).color(icon_color).width(Length::Fixed(40.0)),
                                text(&entry.message).size(13).width(Length::Fill),
                            ]
                            .spacing(8)
                            .padding([4, 8]);

                            col.push(entry_row)
                        });

                let summary = {
                    let errors = entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Error))
                        .count();
                    let warnings = entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Warning))
                        .count();
                    let infos = entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Info))
                        .count();
                    text(format!(
                        "{} error(s), {} warning(s), {} info(s)",
                        errors, warnings, infos
                    ))
                    .size(12)
                };

                column![
                    summary,
                    scrollable(rows.padding(8)).height(Length::Fill),
                ]
                .spacing(8)
                .into()
            }
        }
    };

    column![title_bar, iced::widget::rule::horizontal(1), content]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
