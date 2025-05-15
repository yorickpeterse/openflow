use std::io::{Write, stderr};

#[derive(Copy, Clone)]
pub(crate) enum Level {
    Info,
    Error,
    None,
}

#[derive(Clone)]
pub(crate) struct Logger {
    pub(crate) level: Level,
}

impl Logger {
    pub(crate) fn new() -> Self {
        Self { level: Level::Info }
    }

    pub(crate) fn info(&self, message: String) {
        self.write(Level::Info, message);
    }

    pub(crate) fn error(&self, message: String) {
        self.write(Level::Error, message);
    }

    fn write(&self, level: Level, message: String) {
        let label = match (self.level, level) {
            (Level::Info, Level::Info) => "<6>",
            (Level::Info | Level::Error, Level::Error) => "<3>",
            _ => return,
        };

        let _ = writeln!(stderr(), "{} {}", label, message);
    }
}
