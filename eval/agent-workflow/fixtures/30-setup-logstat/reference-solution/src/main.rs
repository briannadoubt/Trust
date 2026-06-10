//! REFERENCE SOLUTION — never shown to the agent. Strict opt-in comes from
//! `[package.metadata.trust] strict = true` in Cargo.toml — no per-file
//! marker attribute. Named args at both multi-arg call sites; the panic on
//! parse failure now carries a justification message.

#[derive(Debug, Clone, Copy)]
enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug)]
struct LogEntry {
    level: Level,
    message: String,
}

fn parse_entry(line: &str, default_level: Level) -> LogEntry {
    if let Some(rest) = line.strip_prefix("ERROR ") {
        LogEntry {
            level: Level::Error,
            message: rest.to_string(),
        }
    } else if let Some(rest) = line.strip_prefix("WARN ") {
        LogEntry {
            level: Level::Warn,
            message: rest.to_string(),
        }
    } else {
        LogEntry {
            level: default_level,
            message: line.to_string(),
        }
    }
}

fn format_entry(entry: &LogEntry, width: usize) -> String {
    let mut rendered = format!("[{:?}] {}", entry.level, entry.message);
    rendered.truncate(width);
    rendered
}

fn main() {
    let lines = ["ERROR disk full", "WARN low disk", "all good"];
    for line in lines {
        let entry = parse_entry(line: line, default_level: Level::Info);
        println!("{}", format_entry(entry: &entry, width: 32));
    }
    let max_width: usize = "32".parse().expect("string literal is a valid usize");
    println!("max width = {max_width}");
}
