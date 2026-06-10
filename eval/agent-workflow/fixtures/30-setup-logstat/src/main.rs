//! Tiny log formatter. Plain Rust on purpose: the setup task asks the agent
//! to convert this crate to Trust (project-level opt-in, fix what the
//! toolchain reports). Violation surface once strict: two positional
//! multi-arg calls (R0042) and one `.unwrap()` (R0001).

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
        let entry = parse_entry(line, Level::Info);
        println!("{}", format_entry(&entry, 32));
    }
    let max_width: usize = "32".parse().unwrap();
    println!("max width = {max_width}");
}
