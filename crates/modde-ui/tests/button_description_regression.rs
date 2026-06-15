use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn visible_buttons_do_not_use_raw_message_handlers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    collect_raw_button_handlers(&root, &mut offenders);

    assert!(
        offenders.is_empty(),
        "button widgets must use action_button::DescribedButtonExt so hover descriptions are trait-enforced:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn described_button_path_uses_app_level_hover_toasts() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let action_button = [
        fs::read_to_string(manifest_dir.join("src/action_button.rs"))
            .expect("read action_button source"),
        fs::read_to_string(manifest_dir.join("src/action_button_parts/widgets.rs"))
            .expect("read action_button widget source"),
    ]
    .join("\n");
    let app = fs::read_to_string(manifest_dir.join("src/app.rs")).expect("read app source");

    assert!(
        action_button.contains("mouse_area(button)"),
        "DescribedButtonExt must wrap buttons in mouse_area for app-level hover lifecycle messages"
    );
    assert!(
        !action_button.contains("tooltip("),
        "DescribedButtonExt must not use iced::tooltip; tooltips can be clipped by scrollables"
    );
    assert!(
        action_button.contains("Message::ButtonHoverStarted")
            && action_button.contains("Message::ButtonHoverEnded"),
        "DescribedButtonExt must emit hover lifecycle messages"
    );
    assert!(
        app.contains("BUTTON_HOVER_TOAST_DELAY: Duration = Duration::from_secs(2)")
            && app.contains("Message::ButtonHoverElapsed"),
        "Modde must own the delayed 2 second hover toast lifecycle"
    );
}

fn collect_raw_button_handlers(path: &Path, offenders: &mut Vec<String>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read modde-ui src directory") {
            let entry = entry.expect("read modde-ui src entry");
            collect_raw_button_handlers(&entry.path(), offenders);
        }
        return;
    }

    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return;
    }

    if path.file_name().and_then(|name| name.to_str()) == Some("action_button.rs") {
        return;
    }

    let source = fs::read_to_string(path).expect("read Rust source file");
    let lines: Vec<&str> = source.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        if !(line.contains(".on_press(Message::") || line.contains(".on_press_maybe(")) {
            continue;
        }

        if line.contains("Message::TitleBarDrag") {
            continue;
        }

        if surrounding_lines(&lines, index, 4).contains("mouse_area") {
            continue;
        }

        offenders.push(format!("{}:{}", display_path(path), index + 1));
    }
}

fn surrounding_lines(lines: &[&str], index: usize, radius: usize) -> String {
    let start = index.saturating_sub(radius);
    let end = (index + radius + 1).min(lines.len());
    lines[start..end].join("\n")
}

fn display_path(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}
