use crate::models::{AecMode, AppSettings, RecordingItem};
use crate::paths::AppPaths;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub struct Storage {
    connection: Mutex<Connection>,
    paths: AppPaths,
}

impl Storage {
    pub fn open(paths: AppPaths) -> Result<Self> {
        let connection = Connection::open(&paths.database)
            .with_context(|| format!("无法打开数据库 {}", paths.database.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS recordings (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              path TEXT NOT NULL UNIQUE,
              created_at TEXT NOT NULL,
              duration_ms INTEGER NOT NULL,
              size_bytes INTEGER NOT NULL,
              recovered INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              session_id TEXT NOT NULL,
              component TEXT NOT NULL,
              code TEXT NOT NULL,
              occurred_at TEXT NOT NULL,
              detail TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS consents (
              session_id TEXT PRIMARY KEY,
              confirmed_at TEXT NOT NULL,
              template TEXT NOT NULL
            );
            ",
        )?;
        migrate_legacy_recording_paths(&connection, &paths)?;
        Ok(Self {
            connection: Mutex::new(connection),
            paths,
        })
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn settings(&self) -> Result<AppSettings> {
        let connection = self.connection.lock();
        let output_directory = self
            .setting_value(&connection, "output_directory")?
            .unwrap_or_else(|| self.paths.default_recordings.to_string_lossy().into_owned());
        let aec_mode = match self.setting_value(&connection, "aec_mode")?.as_deref() {
            Some("on") => AecMode::On,
            Some("off") => AecMode::Off,
            _ => AecMode::Auto,
        };
        let microphone_enabled = self
            .setting_value(&connection, "microphone_enabled")?
            .map(|value| value == "true")
            .unwrap_or(true);
        let consent_template = self
            .setting_value(&connection, "consent_template")?
            .unwrap_or_else(|| {
                "提示：为了整理本次会议内容，我将在本地录音。录音仅保存在我的电脑中，如有异议请随时告知。".into()
            });
        let first_run_complete = self
            .setting_value(&connection, "first_run_complete")?
            .map(|value| value == "true")
            .unwrap_or(false);
        let recording_notice_acknowledged =
            match self.setting_value(&connection, "recording_notice_acknowledged")? {
                Some(value) => value == "true",
                None => connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM consents LIMIT 1)",
                    [],
                    |row| row.get(0),
                )?,
            };
        let shortcuts_enabled = self
            .setting_value(&connection, "shortcuts_enabled")?
            .map(|value| value == "true")
            .unwrap_or(true);
        let toggle_shortcut = self
            .setting_value(&connection, "toggle_shortcut")?
            .unwrap_or_else(|| "Ctrl+Alt+F9".into());
        let stop_shortcut = self
            .setting_value(&connection, "stop_shortcut")?
            .unwrap_or_else(|| "Ctrl+Alt+F10".into());
        Ok(AppSettings {
            output_directory,
            aec_mode,
            microphone_enabled,
            consent_template,
            first_run_complete,
            recording_notice_acknowledged,
            shortcuts_enabled,
            toggle_shortcut,
            stop_shortcut,
        })
    }

    fn setting_value(&self, connection: &Connection, key: &str) -> Result<Option<String>> {
        Ok(connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let values = [
            ("output_directory", settings.output_directory.clone()),
            (
                "aec_mode",
                match settings.aec_mode {
                    AecMode::Auto => "auto",
                    AecMode::On => "on",
                    AecMode::Off => "off",
                }
                .into(),
            ),
            (
                "microphone_enabled",
                settings.microphone_enabled.to_string(),
            ),
            ("consent_template", settings.consent_template.clone()),
            (
                "first_run_complete",
                settings.first_run_complete.to_string(),
            ),
            (
                "recording_notice_acknowledged",
                settings.recording_notice_acknowledged.to_string(),
            ),
            ("shortcuts_enabled", settings.shortcuts_enabled.to_string()),
            ("toggle_shortcut", settings.toggle_shortcut.clone()),
            ("stop_shortcut", settings.stop_shortcut.clone()),
        ];
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        for (key, value) in values {
            transaction.execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn record_consent(&self, session_id: &str, template: &str) -> Result<()> {
        self.connection.lock().execute(
            "INSERT OR REPLACE INTO consents(session_id, confirmed_at, template)
             VALUES(?1, ?2, ?3)",
            params![session_id, Utc::now().to_rfc3339(), template],
        )?;
        Ok(())
    }

    pub fn insert_recording(&self, item: &RecordingItem) -> Result<()> {
        self.connection.lock().execute(
            "INSERT OR REPLACE INTO recordings
             (id, title, path, created_at, duration_ms, size_bytes, recovered)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                item.id,
                item.title,
                item.path,
                item.created_at,
                item.duration_ms.min(i64::MAX as u64) as i64,
                item.size_bytes.min(i64::MAX as u64) as i64,
                item.recovered as i32
            ],
        )?;
        Ok(())
    }

    pub fn list_recordings(&self) -> Result<Vec<RecordingItem>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, title, path, created_at, duration_ms, size_bytes, recovered
             FROM recordings ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RecordingItem {
                id: row.get(0)?,
                title: row.get(1)?,
                path: row.get(2)?,
                created_at: row.get(3)?,
                duration_ms: row.get::<_, i64>(4)?.max(0) as u64,
                size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
                recovered: row.get::<_, i32>(6)? != 0,
            })
        })?;
        Ok(rows
            .filter_map(Result::ok)
            .filter(|item| Path::new(&item.path).exists())
            .collect())
    }

    pub fn find_recording(&self, id: &str) -> Result<RecordingItem> {
        self.connection
            .lock()
            .query_row(
                "SELECT id, title, path, created_at, duration_ms, size_bytes, recovered
                 FROM recordings WHERE id = ?1",
                [id],
                |row| {
                    Ok(RecordingItem {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        path: row.get(2)?,
                        created_at: row.get(3)?,
                        duration_ms: row.get::<_, i64>(4)?.max(0) as u64,
                        size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
                        recovered: row.get::<_, i32>(6)? != 0,
                    })
                },
            )
            .context("找不到该录音")
    }

    pub fn rename_recording(&self, id: &str, title: &str) -> Result<RecordingItem> {
        let title = sanitize_title(title)?;
        let item = self.find_recording(id)?;
        let old_path = PathBuf::from(&item.path);
        let extension = old_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("ogg");
        let new_path = old_path.with_file_name(format!("{title}.{extension}"));
        std::fs::rename(&old_path, &new_path)?;
        self.connection.lock().execute(
            "UPDATE recordings SET title = ?1, path = ?2 WHERE id = ?3",
            params![title, new_path.to_string_lossy(), id],
        )?;
        self.find_recording(id)
    }

    pub fn remove_recording(&self, id: &str) -> Result<RecordingItem> {
        let item = self.find_recording(id)?;
        self.connection
            .lock()
            .execute("DELETE FROM recordings WHERE id = ?1", [id])?;
        Ok(item)
    }
}

fn migrate_legacy_recording_paths(connection: &Connection, paths: &AppPaths) -> Result<()> {
    if paths.default_recordings == paths.legacy_default_recordings {
        return Ok(());
    }

    let legacy = paths
        .legacy_default_recordings
        .to_string_lossy()
        .into_owned();
    let nota = paths.default_recordings.to_string_lossy().into_owned();
    let output_directory = connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'output_directory'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if output_directory.is_some_and(|value| value.eq_ignore_ascii_case(&legacy)) {
        connection.execute(
            "UPDATE settings SET value = ?1 WHERE key = 'output_directory'",
            [&nota],
        )?;
    }

    let recordings = {
        let mut statement = connection.prepare("SELECT id, path FROM recordings")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.filter_map(std::result::Result::ok).collect::<Vec<_>>()
    };
    for (id, value) in recordings {
        let old_path = PathBuf::from(&value);
        let Ok(relative) = old_path.strip_prefix(&paths.legacy_default_recordings) else {
            continue;
        };
        let new_path = paths.default_recordings.join(relative);
        if new_path.is_file() {
            connection.execute(
                "UPDATE recordings SET path = ?1 WHERE id = ?2",
                params![new_path.to_string_lossy(), id],
            )?;
        }
    }
    Ok(())
}

fn sanitize_title(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("名称不能为空");
    }
    let sanitized: String = trimmed
        .chars()
        .map(|character| {
            if r#"<>:"/\|?*"#.contains(character) {
                '_'
            } else {
                character
            }
        })
        .take(100)
        .collect();
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migration_schema(connection: &Connection) {
        connection
            .execute_batch(
                "
                CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE recordings (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  path TEXT NOT NULL UNIQUE,
                  created_at TEXT NOT NULL,
                  duration_ms INTEGER NOT NULL,
                  size_bytes INTEGER NOT NULL,
                  recovered INTEGER NOT NULL DEFAULT 0
                );
                ",
            )
            .unwrap();
    }

    #[test]
    fn migrates_legacy_default_setting_and_indexed_recording_paths() {
        let root = std::env::temp_dir().join(format!("nota-storage-test-{}", uuid::Uuid::new_v4()));
        let legacy = root.join("Meeting Note").join("Recordings");
        let nota = root.join("Nota").join("Recordings");
        std::fs::create_dir_all(&nota).unwrap();
        let filename = "meeting.ogg";
        std::fs::write(nota.join(filename), b"audio").unwrap();

        let connection = Connection::open_in_memory().unwrap();
        migration_schema(&connection);
        connection
            .execute(
                "INSERT INTO settings(key, value) VALUES('output_directory', ?1)",
                [legacy.to_string_lossy().as_ref()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO recordings
                 (id, title, path, created_at, duration_ms, size_bytes, recovered)
                 VALUES('id', 'meeting', ?1, '2026-01-01T00:00:00Z', 0, 5, 0)",
                [legacy.join(filename).to_string_lossy().as_ref()],
            )
            .unwrap();
        let paths = AppPaths {
            recovery: root.join("Nota").join("Recovery"),
            logs: root.join("Nota").join("Logs"),
            default_recordings: nota.clone(),
            legacy_default_recordings: legacy,
            database: root.join("Nota").join("nota.db"),
        };

        migrate_legacy_recording_paths(&connection, &paths).unwrap();

        let output: String = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'output_directory'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let recording: String = connection
            .query_row("SELECT path FROM recordings WHERE id = 'id'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(PathBuf::from(output), nota);
        assert_eq!(PathBuf::from(recording), nota.join(filename));

        std::fs::remove_file(nota.join(filename)).unwrap();
        std::fs::remove_dir(&nota).unwrap();
        std::fs::remove_dir(nota.parent().unwrap()).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn preserves_a_custom_output_directory_during_brand_migration() {
        let connection = Connection::open_in_memory().unwrap();
        migration_schema(&connection);
        let custom = r"F:\会议录音文件存储库";
        connection
            .execute(
                "INSERT INTO settings(key, value) VALUES('output_directory', ?1)",
                [custom],
            )
            .unwrap();
        let paths = AppPaths {
            recovery: PathBuf::from(r"C:\Users\user\AppData\Local\Nota\Recovery"),
            logs: PathBuf::from(r"C:\Users\user\AppData\Local\Nota\Logs"),
            default_recordings: PathBuf::from(r"C:\Users\user\Documents\Nota\Recordings"),
            legacy_default_recordings: PathBuf::from(
                r"C:\Users\user\Documents\Meeting Note\Recordings",
            ),
            database: PathBuf::from(r"C:\Users\user\AppData\Local\Nota\nota.db"),
        };

        migrate_legacy_recording_paths(&connection, &paths).unwrap();

        let output: String = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'output_directory'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(output, custom);
    }
}
