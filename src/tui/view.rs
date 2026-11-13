use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use crate::fleet::{AgentPane, Connection, Fleet, format_elapsed};
use crate::tty7::types::Status;

const GROUPS: [(Status, &str); 5] = [
    (Status::Waiting, "NEEDS YOU"),
    (Status::Working, "WORKING"),
    (Status::Done, "DONE"),
    (Status::Idle, "IDLE"),
    (Status::Unknown, "OTHER"),
];

fn status_color(status: Status) -> Color {
    match status {
        Status::Waiting => Color::Yellow,
        Status::Working => Color::Blue,
        Status::Done => Color::Green,
        Status::Idle | Status::Unknown => Color::DarkGray,
    }
}

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn render(frame: &mut Frame, fleet: &Fleet, app: &App, now_ms: u64) {
    let banners = banners(fleet);
    let [header, banner_area, list, preview, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(banners.len() as u16),
        Constraint::Min(3),
        Constraint::Percentage(40),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Paragraph::new(header_line(fleet)), header);
    frame.render_widget(Paragraph::new(banners), banner_area);
    frame.render_widget(Paragraph::new(list_lines(fleet, app, now_ms)), list);
    frame.render_widget(
        Paragraph::new(preview_lines(fleet, app, preview.height)),
        preview,
    );
    frame.render_widget(Paragraph::new(footer_line(app)), footer);
}

fn header_line(fleet: &Fleet) -> Line<'static> {
    let mut spans = vec![
        Span::styled("⛵ shipherd", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {} agents", fleet.panes.len()), dim()),
    ];
    let needs_you = fleet.count(Status::Waiting);
    if needs_you > 0 {
        spans.push(Span::styled(
            format!(" · {needs_you} need you"),
            Style::new().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

fn banners(fleet: &Fleet) -> Vec<Line<'static>> {
    let warn = Style::new().fg(Color::Black).bg(Color::Yellow);
    let mut lines: Vec<Line<'static>> = fleet
        .hooks_missing()
        .into_iter()
        .map(|agent| {
            Line::styled(
                format!(" status hooks not installed for {agent}: tty7 Settings → Integrations "),
                warn,
            )
        })
        .collect();
    match fleet.connection {
        Connection::Connected => {}
        Connection::Reconnecting => lines.push(Line::styled(
            " tty7 event stream lost, reconnecting… ",
            warn,
        )),
        Connection::Unreachable => {
            lines.push(Line::styled(" tty7 server not reachable, retrying… ", warn))
        }
    }
    lines
}

fn list_lines(fleet: &Fleet, app: &App, now_ms: u64) -> Vec<Line<'static>> {
    let ordered = fleet.ordered();
    if ordered.is_empty() {
        return vec![Line::styled("no agents running in tty7", dim())];
    }
    let mut lines = Vec::new();
    for (status, title) in GROUPS {
        let group: Vec<&AgentPane> = ordered
            .iter()
            .copied()
            .filter(|p| p.status == status)
            .collect();
        if group.is_empty() {
            continue;
        }
        let heading = Style::new()
            .fg(status_color(status))
            .add_modifier(Modifier::BOLD);
        lines.push(Line::styled(title, heading));
        for pane in group {
            lines.push(agent_line(pane, app.selected == Some(pane.pane), now_ms));
        }
    }
    lines
}

fn agent_line(pane: &AgentPane, selected: bool, now_ms: u64) -> Line<'static> {
    let elapsed = format_elapsed(
        now_ms.saturating_sub(pane.status_since_ms),
        pane.since_exact,
    );
    let text = format!(
        " {:<7} {:<12} {:<14} {:>7}",
        fit(&pane.agent.to_lowercase(), 7),
        fit(pane.repo.as_deref().unwrap_or("-"), 12),
        fit(pane.branch.as_deref().unwrap_or("-"), 14),
        elapsed
    );
    let line = Line::from(vec![
        Span::styled("●", Style::new().fg(status_color(pane.status))),
        Span::raw(text),
    ]);
    if selected {
        line.style(Style::new().bg(Color::Rgb(31, 42, 58)))
    } else {
        line
    }
}

/// `format!` padding doesn't truncate, so long names would push the elapsed time off screen.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        let kept: String = text.chars().take(width - 1).collect();
        format!("{kept}…")
    }
}

fn preview_lines(fleet: &Fleet, app: &App, height: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled("─".repeat(200), dim())];
    let input = app.reply.as_ref().map(|buffer| {
        Line::from(vec![
            Span::styled("reply> ", Style::new().fg(Color::Cyan)),
            Span::raw(format!("{buffer}▏")),
        ])
    });
    let room = (height as usize).saturating_sub(1 + usize::from(input.is_some()));
    let preview = app
        .selected
        .and_then(|id| fleet.panes.get(&id))
        .and_then(|p| p.preview.as_deref())
        .unwrap_or("");
    let all: Vec<&str> = preview.lines().collect();
    // Full-screen agents pad their output with blank lines; show the last lines with content.
    let end = all
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    let start = end.saturating_sub(room);
    lines.extend(all[start..end].iter().map(|l| Line::raw(l.to_string())));
    lines.extend(input);
    lines
}

fn footer_line(app: &App) -> Line<'static> {
    match (&app.flash, &app.reply) {
        (Some(flash), _) => Line::styled(flash.clone(), Style::new().fg(Color::Yellow)),
        (None, Some(_)) => Line::styled("enter send · esc cancel", dim()),
        (None, None) => Line::styled("↑↓ select · y approve · n deny · r reply · q quit", dim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::testing::fleet_with;
    use crate::tty7::types::AgentsResponse;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const NOW: u64 = 1_000_000;

    fn draw(fleet: &Fleet, app: &App) -> String {
        let (width, height) = (60, 20);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, fleet, app, NOW))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample() -> Fleet {
        fleet_with(&[
            (1, "Claude", Status::Waiting, NOW - 125_000),
            (2, "Codex", Status::Working, NOW - 41_000),
        ])
    }

    #[test]
    fn header_counts_agents_and_waiting_ones() {
        let screen = draw(&sample(), &App::default());
        let header = screen.lines().next().unwrap();
        assert!(header.contains("shipherd"), "{screen}");
        assert!(header.contains("2 agents"), "{screen}");
        assert!(header.contains("1 need you"), "{screen}");
    }

    #[test]
    fn groups_appear_in_status_order_with_elapsed_time() {
        let screen = draw(&sample(), &App::default());
        let needs_you = screen.find("NEEDS YOU").expect("NEEDS YOU heading");
        let working = screen.find("WORKING").expect("WORKING heading");
        assert!(needs_you < working, "{screen}");
        assert!(screen.contains("2m05s"), "{screen}");
        assert!(screen.contains("41s"), "{screen}");
        assert!(screen.contains("codex"), "{screen}");
    }

    #[test]
    fn statuses_seen_at_startup_show_a_lower_bound() {
        let mut fleet = Fleet::default();
        let resp: AgentsResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/agents.json")).unwrap();
        fleet.resync(&resp, NOW - 60_000);
        assert!(draw(&fleet, &App::default()).contains("≥1m00s"));
    }

    #[test]
    fn banners_for_missing_hooks_and_lost_connection() {
        let mut fleet = Fleet::default();
        let resp: AgentsResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/agents-no-hooks.json"))
                .unwrap();
        fleet.resync(&resp, NOW);
        fleet.connection = Connection::Reconnecting;
        let screen = draw(&fleet, &App::default());
        assert!(
            screen.contains("status hooks not installed for claude"),
            "{screen}"
        );
        assert!(screen.contains("reconnecting"), "{screen}");
        assert!(screen.contains("no agents running in tty7"), "{screen}");
    }

    #[test]
    fn preview_shows_the_last_lines_of_the_selected_agent() {
        let mut fleet = sample();
        fleet.set_preview(1, "first\nmiddle\nDo you want to proceed?\n\n\n".into());
        let app = App {
            selected: Some(1),
            ..App::default()
        };
        let screen = draw(&fleet, &app);
        assert!(screen.contains("Do you want to proceed?"), "{screen}");
    }

    #[test]
    fn reply_input_and_flash_are_visible() {
        let app = App {
            selected: Some(1),
            reply: Some("hi there".into()),
            ..App::default()
        };
        assert!(draw(&sample(), &app).contains("reply> hi there"));

        let app = App {
            flash: Some("agent moved on (now working)".into()),
            ..App::default()
        };
        assert!(draw(&sample(), &app).contains("agent moved on (now working)"));
    }

    #[test]
    fn footer_lists_the_keys() {
        assert!(draw(&sample(), &App::default()).contains("y approve · n deny · r reply · q quit"));
    }

    #[test]
    fn long_repo_and_branch_names_do_not_push_the_time_off_screen() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Working, NOW - 41_000)]);
        fleet.set_git(
            1,
            Some("hermes-reimagined".into()),
            Some("feature/ari-48899-hermes-us-reimagined-production".into()),
        );
        let screen = draw(&fleet, &App::default());
        let row = screen.lines().find(|l| l.contains("claude")).unwrap();
        assert!(row.contains("41s"), "{screen}");
        assert!(row.contains("hermes-reim…"), "{screen}");
        assert!(row.contains("feature/ari-4…"), "{screen}");
    }
}
