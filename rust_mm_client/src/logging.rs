use {
    chrono::Local,
    log::{Level, LevelFilter, Metadata, Record, SetLoggerError},
};

static LOGGER: Logger = Logger;

const DATE_FORMAT_STR: &str = "%Y-%m-%d %H:%M:%S";

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!(
                "{} - {} - {}",
                Local::now().format(DATE_FORMAT_STR),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

pub fn init_logger() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info))
}
