use std::sync::{Mutex, OnceLock};

static APP_LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

pub fn get_logs() -> &'static Mutex<Vec<String>> {
    APP_LOGS.get_or_init(|| Mutex::new(Vec::new()))
}

pub struct MemoryLogger;

impl log::Log for MemoryLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let msg = format!(
            "[{}] [{}] {}",
            jiff::Zoned::now().strftime("%H:%M:%S"),
            record.level(),
            record.args()
        );
        eprintln!("{}", msg);
        if let Ok(mut logs) = get_logs().lock() {
            logs.push(msg);
            if logs.len() > 1000 {
                logs.remove(0);
            }
        }
    }

    fn flush(&self) {}
}

static LOGGER: MemoryLogger = MemoryLogger;

pub fn init() {
    let _ = log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(log::LevelFilter::Info));
}
