use anyhow::Result;
use log::{LevelFilter, Log, Metadata, Record};
use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const LOG_FILES: usize = 3;

struct RollingLogger {
    directory: PathBuf,
    file: Mutex<File>,
}

impl Log for RollingLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut file = self.file.lock();
        if file
            .metadata()
            .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
            && let Ok(replacement) = rotate(&self.directory)
        {
            *file = replacement;
        }
        let _ = writeln!(
            file,
            "{} {:<5} {}",
            chrono::Utc::now().to_rfc3339(),
            record.level(),
            record.args()
        );
    }

    fn flush(&self) {
        let _ = self.file.lock().flush();
    }
}

pub fn init(directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let file = open_current(directory)?;
    let logger = RollingLogger {
        directory: directory.to_path_buf(),
        file: Mutex::new(file),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    Ok(())
}

fn open_current(directory: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("nota.log"))?)
}

fn rotate(directory: &Path) -> Result<File> {
    let oldest = directory.join(format!("nota.{}.log", LOG_FILES - 1));
    if oldest.exists() {
        std::fs::remove_file(oldest)?;
    }
    for index in (1..LOG_FILES - 1).rev() {
        let source = directory.join(format!("nota.{index}.log"));
        let destination = directory.join(format!("nota.{}.log", index + 1));
        if source.exists() {
            std::fs::rename(source, destination)?;
        }
    }
    let current = directory.join("nota.log");
    if current.exists() {
        std::fs::rename(current, directory.join("nota.1.log"))?;
    }
    open_current(directory)
}
