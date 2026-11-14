use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::{App, Grouping};
use crate::fleet::{AgentPane, Fleet, format_elapsed};
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

    frame.render_widget(
        Paragraph::new(header_line(fleet, app, header.width)),
        header,
    );
    frame.render_widget(Paragraph::new(banners), banner_area);
    let (lines, selected_line) = list_lines(fleet, app, now_ms, list.width);
    let offset = scroll_offset(selected_line, list.height);
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), list);
    frame.render_widget(
        Paragraph::new(preview_lines(fleet, app, preview.height, now_ms)),
        preview,
    );
    frame.render_widget(
        Paragraph::new(footer_line(fleet, app, footer.width)),
        footer,
    );
}

fn header_line(fleet: &Fleet, app: &App, width: u16) -> Line<'static> {
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
    if app.grouping == Grouping::Workspace {
        spans.push(Span::styled("  by workspace", Style::new().fg(Color::Cyan)));
    }
    // A cut-off URL ("…:777") looks valid but points nowhere, so it's all or nothing.
    if let Some(url) = &app.web_url {
        let url = Span::styled(format!("  {url}"), dim());
        if Line::from(spans.clone()).width() + url.width() <= usize::from(width) {
            spans.push(url);
        }
    }
    Line::from(spans)
}

fn banners(fleet: &Fleet) -> Vec<Line<'static>> {
    let warn = Style::new().fg(Color::Black).bg(Color::Yellow);
    fleet
        .banners()
        .into_iter()
        .map(|banner| Line::styled(format!(" {banner} "), warn))
        .collect()
}

/// Returns the lines and the index of the selected agent's line, for scrolling.
fn list_lines(
    fleet: &Fleet,
    app: &App,
    now_ms: u64,
    width: u16,
) -> (Vec<Line<'static>>, Option<usize>) {
    if fleet.panes.is_empty() {
        return (vec![Line::styled("no agents running in tty7", dim())], None);
    }
    // Rows by workspace say the status in the column that otherwise names the workspace.
    let slot = match app.grouping {
        Grouping::Status => Slot::Workspace,
        Grouping::Workspace => Slot::Status,
    };
    let mut selected_line = None;
    let mut lines = Vec::new();
    for (heading, group) in sections(fleet, app.grouping) {
        lines.push(heading);
        for pane in group {
            let selected = app.selected == Some(pane.pane);
            if selected {
                selected_line = Some(lines.len());
            }
            lines.push(agent_line(pane, slot, selected, now_ms, usize::from(width)));
        }
    }
    (lines, selected_line)
}

/// Non-empty sections with their headings, in the order `App::ordered` walks them.
fn sections(fleet: &Fleet, grouping: Grouping) -> Vec<(Line<'static>, Vec<&AgentPane>)> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    match grouping {
        Grouping::Status => {
            let ordered = fleet.ordered();
            GROUPS
                .into_iter()
                .filter_map(|(status, title)| {
                    let group: Vec<&AgentPane> = ordered
                        .iter()
                        .copied()
                        .filter(|p| p.board_status() == status)
                        .collect();
                    (!group.is_empty())
                        .then(|| (Line::styled(title, bold.fg(status_color(status))), group))
                })
                .collect()
        }
        Grouping::Workspace => fleet
            .by_workspace()
            .into_iter()
            .map(|(workspace, group)| {
                let title = format!("{} ({})", workspace.unwrap_or("other"), group.len());
                (Line::styled(title, bold.fg(Color::Cyan)), group)
            })
            .collect(),
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Workspace,
    Status,
}

/// Scrolls just far enough that the selected line is the last visible one.
fn scroll_offset(selected_line: Option<usize>, height: u16) -> u16 {
    match selected_line {
        Some(line) if line >= usize::from(height) => (line + 1 - usize::from(height)) as u16,
        _ => 0,
    }
}

const AGENT_WIDTH: usize = 7;
const TIME_WIDTH: usize = 7;
/// Dot, agent and time, with their separating spaces.
const ROW_BASE_WIDTH: usize = 1 + 1 + AGENT_WIDTH + 1 + TIME_WIDTH;

fn agent_line(
    pane: &AgentPane,
    slot: Slot,
    selected: bool,
    now_ms: u64,
    width: usize,
) -> Line<'static> {
    // Without a reported status, the time since shipherd first saw the agent means nothing.
    let elapsed = if pane.has_status {
        format_elapsed(
            now_ms.saturating_sub(pane.status_since_ms),
            pane.since_exact,
        )
    } else {
        "–".to_string()
    };
    // As the pane narrows, branch goes first, then workspace, then repo; the time always stays.
    let show_repo = width >= ROW_BASE_WIDTH + 13;
    let show_workspace = width >= ROW_BASE_WIDTH + 13 + 13;
    let show_branch = width >= ROW_BASE_WIDTH + 13 + 13 + 15;
    let mut text = format!(
        " {:<AGENT_WIDTH$}",
        fit(&pane.agent.to_lowercase(), AGENT_WIDTH)
    );
    if show_workspace {
        let value = match slot {
            Slot::Workspace => pane.workspace.clone().unwrap_or_else(|| "-".into()),
            Slot::Status => status_text(pane),
        };
        text += &format!(" {:<12}", fit(&value, 12));
    }
    if show_repo {
        text += &format!(" {:<12}", fit(pane.repo.as_deref().unwrap_or("-"), 12));
    }
    if show_branch {
        text += &format!(" {:<14}", fit(pane.branch.as_deref().unwrap_or("-"), 14));
    }
    text += &format!(" {elapsed:>TIME_WIDTH$}");
    let marker = if pane.background.is_empty() {
        "●"
    } else {
        "⟳"
    };
    let line = Line::from(vec![
        Span::styled(marker, Style::new().fg(status_color(pane.board_status()))),
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

/// "done ⟳1" while background agents run after the main turn.
fn status_text(pane: &AgentPane) -> String {
    match pane.background.len() {
        0 => pane.status.label().to_string(),
        n => format!("{} ⟳{n}", pane.status.label()),
    }
}

fn preview_lines(fleet: &Fleet, app: &App, height: u16, now_ms: u64) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled("─".repeat(200), dim())];
    let input = app.reply.as_ref().map(|buffer| {
        Line::from(vec![
            Span::styled("reply> ", Style::new().fg(Color::Cyan)),
            Span::raw(format!("{buffer}▏")),
        ])
    });
    let selected = app.selected.and_then(|id| fleet.panes.get(&id));
    let background: Vec<Line<'static>> = selected
        .map(|p| {
            p.background
                .iter()
                .map(|t| {
                    let age = format_elapsed(now_ms.saturating_sub(t.since_ms), true);
                    Line::styled(
                        format!("⟳ {} · {} · {age}", t.kind, t.description),
                        Style::new().fg(Color::Blue),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let room =
        (height as usize).saturating_sub(1 + usize::from(input.is_some()) + background.len());
    lines.extend(background);
    let preview = selected.and_then(|p| p.preview.as_deref()).unwrap_or("");
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

const KEYS_HINT: &str =
    "↑↓ select · y approve · n deny · r reply · x close · g group · w web · q quit";
const KEYS_HINT_SHORT: &str = "↑↓ y n r x g w q";

fn footer_line(fleet: &Fleet, app: &App, width: u16) -> Line<'static> {
    if let Some(pane) = app.confirm_close {
        let (agent, workspace) = fleet
            .panes
            .get(&pane)
            .map_or(("agent".into(), "-".into()), |p| {
                (
                    p.agent.to_lowercase(),
                    p.workspace.clone().unwrap_or_else(|| "-".into()),
                )
            });
        let question = format!("close %{pane} ({agent} · {workspace})? y/n");
        return Line::styled(question, Style::new().fg(Color::Black).bg(Color::Yellow));
    }
    match (&app.flash, &app.reply) {
        (Some(flash), _) => Line::styled(flash.clone(), Style::new().fg(Color::Yellow)),
        (None, Some(_)) => Line::styled("enter send · esc cancel", dim()),
        (None, None) => {
            let full = Line::styled(KEYS_HINT, dim());
            if full.width() <= usize::from(width) {
                full
            } else {
                Line::styled(KEYS_HINT_SHORT, dim())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::BackgroundTask;
    use crate::fleet::Connection;
    use crate::fleet::testing::fleet_with;
    use crate::tty7::types::AgentsResponse;
    use crate::tui::app::Grouping;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const NOW: u64 = 1_000_000;

    fn draw(fleet: &Fleet, app: &App) -> String {
        draw_sized(fleet, app, 60, 20)
    }

    fn draw_sized(fleet: &Fleet, app: &App, width: u16, height: u16) -> String {
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
        let wide = draw_sized(&sample(), &App::default(), 80, 20);
        assert!(
            wide.contains(
                "↑↓ select · y approve · n deny · r reply · x close · g group · w web · q quit"
            ),
            "{wide}"
        );
        let narrow = draw_sized(&sample(), &App::default(), 40, 20);
        assert!(
            narrow
                .lines()
                .last()
                .unwrap()
                .trim_end()
                .ends_with("↑↓ y n r x g w q"),
            "{narrow}"
        );
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

    #[test]
    fn header_shows_the_web_url() {
        let app = App {
            web_url: Some("http://127.0.0.1:7777".into()),
            ..App::default()
        };
        let screen = draw(&sample(), &app);
        assert!(
            screen
                .lines()
                .next()
                .unwrap()
                .contains("http://127.0.0.1:7777"),
            "{screen}"
        );
    }

    #[test]
    fn rows_show_the_workspace_and_no_time_without_a_status() {
        let mut fleet = sample();
        fleet.set_detected(
            vec![crate::fleet::testing::detected(22, "PR Reviews", "x")],
            NOW - 5_000,
        );
        let screen = draw(&fleet, &App::default());
        let row = screen
            .lines()
            .find(|l| l.contains("PR Reviews"))
            .expect("row for pane 22");
        assert!(row.trim_end().ends_with('–'), "{screen}");
    }

    #[test]
    fn closing_asks_in_the_footer() {
        let mut fleet = sample();
        fleet.set_detected(
            vec![crate::fleet::testing::detected(1, "PR Reviews", "x")],
            NOW,
        );
        let app = App {
            confirm_close: Some(1),
            ..App::default()
        };
        let screen = draw(&fleet, &app);
        assert!(
            screen
                .lines()
                .last()
                .unwrap()
                .contains("close %1 (claude · PR Reviews)? y/n"),
            "{screen}"
        );
    }

    fn many_agents() -> Fleet {
        let panes: Vec<(u32, &str, Status, u64)> = (1..=20)
            .map(|i| (i, "Claude", Status::Idle, NOW - u64::from(i) * 1_000))
            .collect();
        fleet_with(&panes)
    }

    #[test]
    fn the_list_scrolls_to_keep_the_selection_visible() {
        let fleet = many_agents();
        let last = fleet.ordered().last().unwrap().pane;
        let app = App {
            selected: Some(last),
            ..App::default()
        };
        // 14 rows: header, footer, a 40 % preview, so the list has room for about 7 lines.
        let screen = draw_sized(&fleet, &app, 60, 14);
        let elapsed = format_elapsed(NOW - fleet.panes[&last].status_since_ms, true);
        assert!(
            screen.contains(&format!(" {elapsed}")),
            "selected row missing:\n{screen}"
        );
    }

    #[test]
    fn narrow_panes_drop_branch_then_workspace_then_repo_but_keep_the_time() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Working, NOW - 41_000)]);
        fleet.set_detected(
            vec![crate::fleet::testing::detected(1, "PR Reviews", "x")],
            NOW,
        );
        fleet.set_git(1, Some("hermes".into()), Some("main".into()));
        let row = |width| {
            let screen = draw_sized(&fleet, &App::default(), width, 12);
            screen
                .lines()
                .find(|l| l.contains("claude"))
                .unwrap()
                .to_string()
        };
        let has = |r: &str, parts: [&str; 4]| parts.map(|p| r.contains(p));
        assert_eq!(
            has(&row(60), ["PR Reviews", "hermes", "main", "41s"]),
            [true, true, true, true]
        );
        assert_eq!(
            has(&row(50), ["PR Reviews", "hermes", "main", "41s"]),
            [true, true, false, true]
        );
        assert_eq!(
            has(&row(40), ["PR Reviews", "hermes", "main", "41s"]),
            [false, true, false, true]
        );
        assert_eq!(
            has(&row(25), ["PR Reviews", "hermes", "main", "41s"]),
            [false, false, false, true]
        );
    }

    #[test]
    fn by_workspace_sections_are_titled_with_counts_and_rows_show_the_status() {
        let mut fleet = fleet_with(&[
            (1, "Claude", Status::Waiting, NOW - 125_000),
            (2, "Claude", Status::Working, NOW - 41_000),
            (3, "Claude", Status::Done, NOW - 10_000),
        ]);
        fleet.set_detected(
            vec![
                crate::fleet::testing::detected(1, "reviews", "a"),
                crate::fleet::testing::detected(2, "Default", "b"),
                crate::fleet::testing::detected(3, "reviews", "c"),
            ],
            NOW,
        );
        let app = App {
            grouping: Grouping::Workspace,
            ..App::default()
        };
        let screen = draw(&fleet, &app);
        let default = screen.find("Default (1)").expect("Default heading");
        let reviews = screen.find("reviews (2)").expect("reviews heading");
        assert!(default < reviews, "{screen}");
        assert!(!screen.contains("NEEDS YOU"), "{screen}");
        let row = |time: &str| screen.lines().find(|l| l.contains(time)).unwrap();
        assert!(row("2m05s").contains("needs you"), "{screen}");
        assert!(row("41s").contains("working"), "{screen}");
        assert!(!row("41s").contains("Default"), "{screen}");
    }

    #[test]
    fn the_header_says_when_grouped_by_workspace() {
        let app = App {
            grouping: Grouping::Workspace,
            ..App::default()
        };
        let header = draw(&sample(), &app).lines().next().unwrap().to_string();
        assert!(header.contains("by workspace"), "{header}");
        let header = draw(&sample(), &App::default())
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert!(!header.contains("by workspace"), "{header}");
    }

    #[test]
    fn the_header_leaves_out_the_url_rather_than_cutting_it() {
        let app = App {
            web_url: Some("http://127.0.0.1:7777".into()),
            ..App::default()
        };
        let narrow = draw_sized(&sample(), &app, 44, 12);
        let header = narrow.lines().next().unwrap();
        assert!(!header.contains("http"), "{header}");
        assert!(header.contains("1 need you"), "{header}");
    }

    fn with_background(fleet: &mut Fleet, pane: u32) {
        let task = BackgroundTask {
            kind: "researcher".into(),
            description: "Locating prompts".into(),
            since_ms: NOW - 237_000,
        };
        fleet.set_background(&std::collections::HashMap::from([(pane, vec![task])]));
    }

    #[test]
    fn finished_agents_with_background_work_are_listed_under_working() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, NOW - 10_000)]);
        with_background(&mut fleet, 1);
        let screen = draw(&fleet, &App::default());
        assert!(screen.contains("WORKING"), "{screen}");
        assert!(!screen.contains("DONE"), "{screen}");
        assert!(screen.contains('⟳'), "{screen}");
    }

    #[test]
    fn the_preview_lists_background_agents_first() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, NOW - 10_000)]);
        with_background(&mut fleet, 1);
        fleet.set_preview(1, "last output line".into());
        let app = App {
            selected: Some(1),
            ..App::default()
        };
        let screen = draw(&fleet, &app);
        assert!(
            screen.contains("⟳ researcher · Locating prompts · 3m57s"),
            "{screen}"
        );
        assert!(screen.contains("last output line"), "{screen}");
    }

    #[test]
    fn by_workspace_rows_say_done_with_background_count() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Done, NOW - 10_000)]);
        with_background(&mut fleet, 1);
        let app = App {
            grouping: Grouping::Workspace,
            ..App::default()
        };
        let screen = draw(&fleet, &app);
        assert!(screen.contains("done ⟳1"), "{screen}");
    }
}
