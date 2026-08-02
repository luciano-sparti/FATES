use crate::error::Error;
use crate::state::State;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

/// One row of the dashboard: live metrics gathered from the process table.
#[derive(Clone)]
pub struct Row {
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub cpu: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub cmd: String,
    pub cwd: Option<String>,
}

/// Aggregate stats shown in the footer of the dashboard.
#[derive(Default)]
pub struct Totals {
    pub running: usize,
    pub total_cpu: f32,
    pub total_memory_bytes: u64,
}

const HEADERS: [&str; 6] = ["NAME", "STATUS", "PID", "CPU%", "MEMORY", "LIFETIME"];
const MIN_WIDTHS: [usize; 6] = [15, 10, 8, 8, 10, 12];

fn fmt_cpu(cpu: Option<f32>) -> String {
    match cpu {
        Some(c) => format!("{:.1}%", c),
        None => "-".to_string(),
    }
}

fn fmt_mem(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
        None => "-".to_string(),
    }
}

fn fmt_lifetime(secs: Option<u64>) -> String {
    match secs {
        Some(secs) => {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            let secs = secs % 60;
            if hours > 0 {
                format!("{}h {}m {}s", hours, mins, secs)
            } else if mins > 0 {
                format!("{}m {}s", mins, secs)
            } else {
                format!("{}s", secs)
            }
        }
        None => "-".to_string(),
    }
}

/// The plain, ANSI-free cell strings for every row (including the "▸ "
/// selection marker on the chosen row). Widths are computed from these, so
/// box borders and column padding always line up.
fn cell_rows(rows: &[Row], selected: Option<usize>) -> Vec<[String; 6]> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let name = if Some(i) == selected {
                format!("▸ {}", r.name)
            } else {
                r.name.clone()
            };
            [
                name,
                if r.running {
                    "RUNNING".into()
                } else {
                    "STOPPED".into()
                },
                r.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                fmt_cpu(r.cpu),
                fmt_mem(r.memory_bytes),
                fmt_lifetime(r.uptime_seconds),
            ]
        })
        .collect()
}

fn compute_widths(cells: &[[String; 6]]) -> [usize; 6] {
    let mut widths = MIN_WIDTHS;
    for row in cells {
        for (col, cell) in row.iter().enumerate() {
            widths[col] = widths[col].max(cell.chars().count());
        }
    }
    widths
}

fn pad(cell: &str, width: usize) -> String {
    let len = cell.chars().count();
    if len >= width {
        cell.to_string()
    } else {
        format!("{}{}", cell, " ".repeat(width - len))
    }
}

/// `render_plain` is the byte-stable format used when stdout is not a
/// terminal (pipes, tests, scripts). Space-separated columns, no ANSI.
pub fn render_plain(rows: &[Row], totals: &Totals) -> String {
    let cells = cell_rows(rows, None);
    let widths = compute_widths(&cells);

    let mut out = String::new();
    for (col, header) in HEADERS.iter().enumerate() {
        out.push_str(&pad(header, widths[col]));
        if col < HEADERS.len() - 1 {
            out.push(' ');
        }
    }
    out.push('\n');
    out.push_str(&"-".repeat(widths.iter().sum::<usize>() + widths.len().saturating_sub(1)));
    out.push('\n');

    for row in &cells {
        for (col, cell) in row.iter().enumerate() {
            out.push_str(&pad(cell, widths[col]));
            if col < HEADERS.len() - 1 {
                out.push(' ');
            }
        }
        out.push('\n');
    }

    out.push_str(&totals_line(totals));
    out.push('\n');
    out
}

fn styled(text: &str, codes: &str) -> String {
    format!("\x1b[{}m{}\x1b[0m", codes, text)
}

fn border_line(left: char, mid: char, right: char, widths: &[usize]) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        s.push_str(&"─".repeat(*w));
        if i < widths.len() - 1 {
            s.push(mid);
        }
    }
    s.push(right);
    s
}

fn status_style(running: bool) -> &'static str {
    if running { "32" } else { "31" }
}

fn cpu_style(cpu: Option<f32>) -> &'static str {
    match cpu {
        Some(c) if c >= 100.0 => "1;31",
        Some(c) if c >= 50.0 => "1;33",
        Some(_) => "",
        None => "90",
    }
}

fn style_cell(cell: &str, kind: CellKind) -> String {
    let codes = match kind {
        CellKind::Name => "1",
        CellKind::Status(running) => status_style(running),
        CellKind::Pid(Some(_)) => "36",
        CellKind::Pid(None) => "90",
        CellKind::Cpu => cpu_style(cpu_from_cell(cell)),
        CellKind::Memory => "",
        CellKind::Lifetime => "",
    };
    if codes.is_empty() {
        cell.to_string()
    } else {
        styled(cell, codes)
    }
}

fn cpu_from_cell(cell: &str) -> Option<f32> {
    cell.trim_end_matches('%').parse::<f32>().ok()
}

enum CellKind {
    Name,
    Status(bool),
    Pid(Option<u32>),
    Cpu,
    Memory,
    Lifetime,
}

/// Box-drawn, ANSI-colored dashboard for terminals. `selected` highlights one
/// row with reverse video (used by the interactive watch loop).
pub fn render_colored(rows: &[Row], totals: &Totals, selected: Option<usize>) -> String {
    let cells = cell_rows(rows, selected);
    let widths = compute_widths(&cells);

    let mut out = String::new();
    out.push_str(&border_line('┌', '┬', '┐', &widths));
    out.push('\n');

    let header_line = HEADERS
        .iter()
        .enumerate()
        .map(|(col, h)| styled(&pad(h, widths[col]), "1"))
        .collect::<Vec<_>>()
        .join("│");
    out.push('│');
    out.push_str(&header_line);
    out.push_str("│\n");

    out.push_str(&border_line('├', '┼', '┤', &widths));
    out.push('\n');

    for (i, row) in rows.iter().enumerate() {
        let mut styled_cells: Vec<String> = Vec::with_capacity(6);
        for col in 0..6 {
            let cell = &cells[i][col];
            let kind = match col {
                0 => CellKind::Name,
                1 => CellKind::Status(row.running),
                2 => CellKind::Pid(row.pid),
                3 => CellKind::Cpu,
                4 => CellKind::Memory,
                _ => CellKind::Lifetime,
            };
            styled_cells.push(style_cell(&pad(cell, widths[col]), kind));
        }
        let line = format!("│{}│", styled_cells.join("│"));
        if Some(i) == selected {
            out.push_str(&styled(&line, "7"));
        } else {
            out.push_str(&line);
        }
        out.push('\n');
    }

    out.push_str(&border_line('├', '┼', '┤', &widths));
    out.push('\n');

    let total_width: usize = widths.iter().sum::<usize>() + widths.len().saturating_sub(1);
    let totals_line = format!(" Totals: {}", totals_line(totals));
    out.push_str(&styled(
        &format!(
            "│{}{}│",
            totals_line,
            " ".repeat(total_width.saturating_sub(totals_line.chars().count()))
        ),
        "1",
    ));
    out.push('\n');

    out.push_str(&border_line('└', '┴', '┘', &widths));
    out.push('\n');
    out
}

fn totals_line(totals: &Totals) -> String {
    let mem = totals.total_memory_bytes as f64 / 1024.0 / 1024.0;
    if totals.running == 0 {
        format!(
            "No running groups · {:.1}% CPU · {:.1} MB",
            totals.total_cpu, mem
        )
    } else {
        format!(
            "{} running · {:.1}% CPU · {:.1} MB",
            totals.running, totals.total_cpu, mem
        )
    }
}

fn key_hint(selected_name: Option<&str>) -> String {
    let name = selected_name.unwrap_or("-");
    styled(
        &format!(
            "↑/↓ select · Enter/L log pane · q quit   [selected: {}]",
            name
        ),
        "90",
    )
}

/// Bottom pane showing the last ~15 lines of the selected group's log.
fn render_log_pane(name: &str, state_dir: &Path) -> String {
    let log_path = State::log_file(state_dir, name);
    let mut out = String::new();
    out.push_str(&styled(&format!("── {} — last 15 lines ──", name), "1;36"));
    out.push('\n');
    if log_path.exists() {
        match crate::commands::tail_bytes(&log_path, 15) {
            Ok(buf) => {
                let text = String::from_utf8_lossy(&buf);
                for line in text.lines() {
                    out.push_str(&styled(line, "2"));
                    out.push('\n');
                }
            }
            Err(e) => {
                out.push_str(&styled(&format!("<could not read log: {}>", e), "31"));
                out.push('\n');
            }
        }
    } else {
        out.push_str(&styled(
            &format!("<no log yet: {}>", log_path.display()),
            "31",
        ));
        out.push('\n');
    }
    out
}

/// Interactive `loom --watch`: alternate screen, live refresh every
/// `interval`, and keyboard navigation. Runs only when stdout is a terminal.
pub fn run_watch(interval: Duration, state_dir: &Path) -> Result<(), Error> {
    use crossterm::cursor::{Hide, Show};
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };

    if !std::io::stdout().is_terminal() {
        return Err(Error::system("interactive watch requires a terminal"));
    }

    if let Err(e) = enable_raw_mode() {
        return Err(Error::system(format!("failed to enter raw mode: {}", e)));
    }
    let mut stdout = std::io::stdout();
    let result: Result<(), Error> = (|| {
        crossterm::execute!(stdout, EnterAlternateScreen, Hide)
            .map_err(|e| Error::system(format!("failed to enter alternate screen: {}", e)))?;

        let mut selected = 0usize;
        let mut log_pane = false;

        loop {
            let (rows, totals) = match crate::commands::loom_rows(state_dir)? {
                Some((rows, totals, _)) => (rows, totals),
                None => (Vec::new(), Totals::default()),
            };

            let mut buf = String::new();
            buf.push_str("\x1b[2J\x1b[H");
            if rows.is_empty() {
                buf.push_str("No process groups found in state.\n");
            } else {
                if selected >= rows.len() {
                    selected = 0;
                }
                buf.push_str(&render_colored(&rows, &totals, Some(selected)));
                if log_pane {
                    buf.push_str(&render_log_pane(&rows[selected].name, state_dir));
                }
                buf.push_str(&key_hint(Some(&rows[selected].name)));
            }
            stdout.write_all(buf.as_bytes()).ok();
            stdout.flush().ok();

            if event::poll(interval)
                .map_err(|e| Error::system(format!("failed to poll for input: {}", e)))?
                && let Event::Key(k) = event::read()
                    .map_err(|e| Error::system(format!("failed to read key: {}", e)))?
            {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected = (selected + 1).min(rows.len().saturating_sub(1))
                    }
                    KeyCode::Enter
                    | KeyCode::Char(' ')
                    | KeyCode::Char('l')
                    | KeyCode::Char('L') => log_pane = !log_pane,
                    _ => {}
                }
            }
        }
        Ok(())
    })();

    let _ = disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), LeaveAlternateScreen, Show);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rows() -> (Vec<Row>, Totals) {
        let rows = vec![
            Row {
                name: "api".to_string(),
                running: true,
                pid: Some(1234),
                cpu: Some(12.5),
                memory_bytes: Some(2 * 1024 * 1024),
                uptime_seconds: Some(63),
                cmd: "node server.js".to_string(),
                cwd: Some("/srv/api".to_string()),
            },
            Row {
                name: "db".to_string(),
                running: false,
                pid: None,
                cpu: None,
                memory_bytes: None,
                uptime_seconds: None,
                cmd: "postgres".to_string(),
                cwd: None,
            },
        ];
        let totals = Totals {
            running: 1,
            total_cpu: 12.5,
            total_memory_bytes: 2 * 1024 * 1024,
        };
        (rows, totals)
    }

    #[test]
    fn plain_renders_headers_rows_and_totals() {
        let (rows, totals) = sample_rows();
        let out = render_plain(&rows, &totals);
        assert!(out.contains("NAME"), "missing NAME header");
        assert!(out.contains("STATUS"), "missing STATUS header");
        assert!(out.contains("RUNNING"), "missing RUNNING status");
        assert!(out.contains("STOPPED"), "missing STOPPED status");
        assert!(out.contains("2.0 MB"), "missing memory cell");
        assert!(out.contains("1m 3s"), "missing lifetime cell");
        assert!(out.contains("1 running"), "missing totals row");
        assert!(out.contains("12.5% CPU"), "missing total CPU");
        assert!(!out.contains('\x1b'), "plain output must not contain ANSI");
    }

    #[test]
    fn colored_uses_box_chars_and_ansi() {
        let (rows, totals) = sample_rows();
        let out = render_colored(&rows, &totals, None);
        assert!(out.contains('┌'), "missing top border");
        assert!(out.contains('└'), "missing bottom border");
        assert!(out.contains("│"), "missing vertical separators");
        assert!(out.contains("\x1b[32m"), "RUNNING should be green");
        assert!(out.contains("\x1b[31m"), "STOPPED should be red");
        assert!(out.contains("Totals"), "missing totals row");
    }

    #[test]
    fn colored_highlights_selected_row() {
        let (rows, totals) = sample_rows();
        let out = render_colored(&rows, &totals, Some(1));
        assert!(
            out.contains("▸ db"),
            "selected row name should carry the marker: {}",
            out
        );
        assert!(out.contains("\x1b[7m"), "selected row should be reversed");
    }

    #[test]
    fn cpu_style_tints_high_usage() {
        assert_eq!(cpu_style(Some(120.0)), "1;31");
        assert_eq!(cpu_style(Some(60.0)), "1;33");
        assert_eq!(cpu_style(Some(10.0)), "");
        assert_eq!(cpu_style(None), "90");
    }
}
