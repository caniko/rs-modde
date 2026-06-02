use crate::views::selectable_text::text;
use iced::widget::{button, column, container, row, scrollable};
use iced::{Alignment, Element, Length, color};
use std::path::PathBuf;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;

/// A single diagnostic finding.
#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

/// Staged profile integrity results included in the diagnostics report.
#[derive(Debug, Clone, Default)]
pub struct IntegritySummary {
    pub ok_count: usize,
    pub broken_symlinks: Vec<PathBuf>,
}

/// Combined diagnostics and integrity report for the active profile.
#[derive(Debug, Clone)]
pub struct DiagnosticsReport {
    pub profile_name: String,
    pub game_id: String,
    pub entries: Vec<DiagnosticEntry>,
    pub integrity: IntegritySummary,
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
    Error(String),
    Complete(DiagnosticsReport),
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
        .style(button::primary)
        .padding([6, 14])
        .on_action_maybe(
            (!running).then_some(ButtonAction::RunDiagnostics),
            "Diagnostics are already running.",
        ),
    ]
    .align_y(Alignment::Center);

    let content: Element<Message> = match state {
        DiagnosticsState::Idle => {
            container(text("Diagnostics run automatically when this view opens.").size(14))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
        }

        DiagnosticsState::Running => container(text("Running diagnostics...").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),

        DiagnosticsState::Error(message) => {
            container(text(message).size(14).color(color!(0xFF4444)))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
        }

        DiagnosticsState::Complete(report) => {
            let broken_count = report.integrity.broken_symlinks.len();
            if report.entries.is_empty() && broken_count == 0 {
                let content = column![
                    text(format!(
                        "Profile: {} ({})",
                        report.profile_name, report.game_id
                    ))
                    .size(12),
                    text(format!("{} staged file(s) OK", report.integrity.ok_count))
                        .size(14)
                        .color(color!(0x88CC88)),
                    text("No issues found!").size(14).color(color!(0x88CC88)),
                ]
                .spacing(8);

                container(content)
                    .padding(20)
                    .width(Length::Fill)
                    .center_x(Length::Fill)
                    .into()
            } else {
                let rows = report
                    .entries
                    .iter()
                    .fold(column![].spacing(4), |col, entry| {
                        let (icon, icon_color) = match entry.severity {
                            DiagnosticSeverity::Info => ("INFO", color!(0x88AACC)),
                            DiagnosticSeverity::Warning => ("WARN", color!(0xFFAA44)),
                            DiagnosticSeverity::Error => ("ERR ", color!(0xFF4444)),
                        };

                        let entry_row = row![
                            text(icon)
                                .size(12)
                                .color(icon_color)
                                .width(Length::Fixed(40.0)),
                            text(&entry.message).size(13).width(Length::Fill),
                        ]
                        .spacing(8)
                        .padding([4, 8]);

                        col.push(entry_row)
                    });

                let summary = {
                    let errors = report
                        .entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Error))
                        .count();
                    let warnings = report
                        .entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Warning))
                        .count();
                    let infos = report
                        .entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Info))
                        .count();
                    text(format!(
                        "{} ({}) - {errors} error(s), {warnings} warning(s), {infos} info(s), {broken_count} broken symlink(s)",
                        report.profile_name, report.game_id
                    ))
                    .size(12)
                };

                let integrity = if report.integrity.broken_symlinks.is_empty() {
                    column![
                        text(format!(
                            "Integrity: {} staged file(s) OK",
                            report.integrity.ok_count
                        ))
                        .size(14)
                        .color(color!(0x88CC88))
                    ]
                } else {
                    let symlinks = report
                        .integrity
                        .broken_symlinks
                        .iter()
                        .fold(column![].spacing(2), |col, path| {
                            col.push(text(path.display().to_string()).size(12))
                        });
                    column![
                        text(format!(
                            "Integrity: {} staged file(s) OK, {} broken symlink(s)",
                            report.integrity.ok_count,
                            report.integrity.broken_symlinks.len()
                        ))
                        .size(14)
                        .color(color!(0xFF8844)),
                        container(symlinks)
                            .padding(8)
                            .width(Length::Fill)
                            .style(container::rounded_box),
                    ]
                    .spacing(6)
                };

                column![
                    summary,
                    integrity,
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
