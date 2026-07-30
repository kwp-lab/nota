use crate::models::{
    AecMode, AppSettings, AsrApiKeyUpdate, AsrProvider, AsrProviderCredentials, AsrProviderKind,
    AsrProviderProbeRequest, RecordingItem, SaveAsrProviderRequest, StoredTranscriptionChunk,
    TranscriptDocument, TranscriptSegment, TranscriptionStatus, TranscriptionSummary,
};
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
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "secure_delete", "ON")?;
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
            CREATE TABLE IF NOT EXISTS asr_providers (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              kind TEXT NOT NULL,
              base_url TEXT NOT NULL,
              api_key TEXT NOT NULL DEFAULT '',
              model_id TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS transcriptions (
              recording_id TEXT PRIMARY KEY REFERENCES recordings(id) ON DELETE CASCADE,
              generation INTEGER NOT NULL DEFAULT 1,
              provider_id TEXT,
              provider_name TEXT NOT NULL,
              model_id TEXT NOT NULL,
              status TEXT NOT NULL,
              completed_chunks INTEGER NOT NULL DEFAULT 0,
              total_chunks INTEGER NOT NULL DEFAULT 0,
              text TEXT NOT NULL DEFAULT '',
              segments_json TEXT NOT NULL DEFAULT '[]',
              language TEXT,
              error_message TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS transcription_chunks (
              recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
              generation INTEGER NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_ms INTEGER NOT NULL,
              end_ms INTEGER NOT NULL,
              text TEXT NOT NULL,
              segments_json TEXT NOT NULL DEFAULT '[]',
              language TEXT,
              completed_at TEXT NOT NULL,
              PRIMARY KEY(recording_id, generation, chunk_index)
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
        let active_asr_provider_id = self
            .setting_value(&connection, "active_asr_provider_id")?
            .filter(|value| !value.trim().is_empty());
        let auto_transcribe = self
            .setting_value(&connection, "auto_transcribe")?
            .map(|value| value == "true")
            .unwrap_or(false);
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
            active_asr_provider_id,
            auto_transcribe,
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
            (
                "active_asr_provider_id",
                settings.active_asr_provider_id.clone().unwrap_or_default(),
            ),
            ("auto_transcribe", settings.auto_transcribe.to_string()),
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
            "INSERT INTO recordings
             (id, title, path, created_at, duration_ms, size_bytes, recovered)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               path = excluded.path,
               created_at = excluded.created_at,
               duration_ms = excluded.duration_ms,
               size_bytes = excluded.size_bytes,
               recovered = excluded.recovered",
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
            "SELECT r.id, r.title, r.path, r.created_at, r.duration_ms, r.size_bytes, r.recovered,
                    t.status, t.completed_chunks, t.total_chunks, t.provider_name, t.model_id,
                    t.error_message, t.text
             FROM recordings r
             LEFT JOIN transcriptions t ON t.recording_id = r.id
             ORDER BY r.created_at DESC",
        )?;
        let rows = statement.query_map([], recording_from_row)?;
        Ok(rows
            .filter_map(Result::ok)
            .filter(|item| Path::new(&item.path).exists())
            .collect())
    }

    pub fn find_recording(&self, id: &str) -> Result<RecordingItem> {
        self.connection
            .lock()
            .query_row(
                "SELECT r.id, r.title, r.path, r.created_at, r.duration_ms, r.size_bytes, r.recovered,
                        t.status, t.completed_chunks, t.total_chunks, t.provider_name, t.model_id,
                        t.error_message, t.text
                 FROM recordings r
                 LEFT JOIN transcriptions t ON t.recording_id = r.id
                 WHERE r.id = ?1",
                [id],
                recording_from_row,
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

    pub fn list_asr_providers(&self) -> Result<Vec<AsrProvider>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, kind, base_url, model_id, api_key != '', created_at, updated_at
             FROM asr_providers ORDER BY name COLLATE NOCASE, created_at",
        )?;
        let rows = statement.query_map([], provider_from_row)?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn find_asr_provider(&self, id: &str) -> Result<AsrProviderCredentials> {
        self.connection
            .lock()
            .query_row(
                "SELECT id, name, kind, base_url, model_id, api_key, created_at, updated_at
                 FROM asr_providers WHERE id = ?1",
                [id],
                |row| {
                    let api_key: String = row.get(5)?;
                    Ok(AsrProviderCredentials {
                        provider: AsrProvider {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            kind: AsrProviderKind::from_str(&row.get::<_, String>(2)?),
                            base_url: row.get(3)?,
                            model_id: row.get(4)?,
                            has_api_key: !api_key.is_empty(),
                            created_at: row.get(6)?,
                            updated_at: row.get(7)?,
                        },
                        api_key,
                    })
                },
            )
            .context("找不到该语音转写服务")
    }

    pub fn asr_probe_credentials(
        &self,
        request: AsrProviderProbeRequest,
    ) -> Result<AsrProviderCredentials> {
        let existing = match request
            .id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(id) => Some(self.find_asr_provider(id)?),
            None => None,
        };
        let api_key = match request.api_key {
            AsrApiKeyUpdate::Keep => existing
                .as_ref()
                .map(|credentials| credentials.api_key.clone())
                .unwrap_or_default(),
            AsrApiKeyUpdate::Replace { value } => value.trim().to_owned(),
            AsrApiKeyUpdate::Clear => String::new(),
        };
        let now = Utc::now().to_rfc3339();
        Ok(AsrProviderCredentials {
            provider: AsrProvider {
                id: existing
                    .as_ref()
                    .map(|credentials| credentials.provider.id.clone())
                    .unwrap_or_default(),
                name: existing
                    .as_ref()
                    .map(|credentials| credentials.provider.name.clone())
                    .unwrap_or_else(|| "未保存的语音转写服务".into()),
                kind: request.kind,
                base_url: request.base_url,
                model_id: request.model_id,
                has_api_key: !api_key.is_empty(),
                created_at: existing
                    .as_ref()
                    .map(|credentials| credentials.provider.created_at.clone())
                    .unwrap_or_else(|| now.clone()),
                updated_at: now,
            },
            api_key,
        })
    }

    pub fn save_asr_provider(&self, request: SaveAsrProviderRequest) -> Result<AsrProvider> {
        let now = Utc::now().to_rfc3339();
        let id = request
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut connection = self.connection.lock();
        let existing = connection
            .query_row(
                "SELECT api_key, created_at FROM asr_providers WHERE id = ?1",
                [&id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let api_key = match request.api_key {
            AsrApiKeyUpdate::Keep => existing
                .as_ref()
                .map(|value| value.0.clone())
                .unwrap_or_default(),
            AsrApiKeyUpdate::Replace { value } => value.trim().to_owned(),
            AsrApiKeyUpdate::Clear => String::new(),
        };
        let created_at = existing.map(|value| value.1).unwrap_or_else(|| now.clone());
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO asr_providers
             (id, name, kind, base_url, api_key, model_id, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               kind = excluded.kind,
               base_url = excluded.base_url,
               api_key = excluded.api_key,
               model_id = excluded.model_id,
               updated_at = excluded.updated_at",
            params![
                id,
                request.name,
                request.kind.as_str(),
                request.base_url,
                api_key,
                request.model_id,
                created_at,
                now
            ],
        )?;
        transaction.commit()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(connection);
        Ok(self.find_asr_provider(&id)?.provider)
    }

    pub fn delete_asr_provider(&self, id: &str) -> Result<()> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let active_jobs: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM transcriptions
             WHERE provider_id = ?1 AND status IN ('queued', 'preparing', 'transcribing')",
            [id],
            |row| row.get(0),
        )?;
        if active_jobs > 0 {
            bail!("该服务仍有正在排队或转写的任务，请先取消任务");
        }
        transaction.execute("DELETE FROM asr_providers WHERE id = ?1", [id])?;
        transaction.execute(
            "UPDATE settings SET value = '' WHERE key = 'active_asr_provider_id' AND value = ?1",
            [id],
        )?;
        transaction.execute(
            "UPDATE settings SET value = 'false' WHERE key = 'auto_transcribe'
             AND NOT EXISTS(SELECT 1 FROM settings WHERE key = 'active_asr_provider_id' AND value != '')",
            [],
        )?;
        transaction.commit()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn begin_transcription(&self, recording_id: &str, provider: &AsrProvider) -> Result<u32> {
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let current_generation = transaction
            .query_row(
                "SELECT generation FROM transcriptions WHERE recording_id = ?1",
                [recording_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        let generation = current_generation.saturating_add(1).max(1);
        transaction.execute(
            "INSERT INTO transcriptions
             (recording_id, generation, provider_id, provider_name, model_id, status,
              completed_chunks, total_chunks, text, segments_json, language,
              error_message, created_at, updated_at, completed_at)
             VALUES(?1, ?2, ?3, ?4, ?5, 'queued', 0, 0, '', '[]', NULL, NULL, ?6, ?6, NULL)
             ON CONFLICT(recording_id) DO UPDATE SET
               generation = excluded.generation,
               provider_id = excluded.provider_id,
               provider_name = excluded.provider_name,
               model_id = excluded.model_id,
               status = 'queued',
               completed_chunks = 0,
               total_chunks = 0,
               error_message = NULL,
               updated_at = excluded.updated_at,
               completed_at = NULL",
            params![
                recording_id,
                generation,
                provider.id,
                provider.name,
                provider.model_id,
                now
            ],
        )?;
        transaction.execute(
            "DELETE FROM transcription_chunks WHERE recording_id = ?1",
            [recording_id],
        )?;
        transaction.commit()?;
        Ok(generation.min(u32::MAX as i64) as u32)
    }

    pub fn resume_transcription(&self, recording_id: &str) -> Result<u32> {
        let connection = self.connection.lock();
        let generation = connection.query_row(
            "SELECT generation FROM transcriptions
             WHERE recording_id = ?1 AND status IN ('failed', 'interrupted', 'cancelled')",
            [recording_id],
            |row| row.get::<_, i64>(0),
        )?;
        connection.execute(
            "UPDATE transcriptions SET status = 'queued', error_message = NULL, updated_at = ?2
             WHERE recording_id = ?1",
            params![recording_id, Utc::now().to_rfc3339()],
        )?;
        Ok(generation.max(1).min(u32::MAX as i64) as u32)
    }

    pub fn set_transcription_progress(
        &self,
        recording_id: &str,
        generation: u32,
        status: TranscriptionStatus,
        completed_chunks: u32,
        total_chunks: u32,
    ) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET status = ?3, completed_chunks = ?4, total_chunks = ?5, updated_at = ?6
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                status.as_str(),
                completed_chunks,
                total_chunks,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn save_transcription_chunk(
        &self,
        recording_id: &str,
        generation: u32,
        chunk: &StoredTranscriptionChunk,
    ) -> Result<()> {
        let segments_json = serde_json::to_string(&chunk.segments)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT OR REPLACE INTO transcription_chunks
             (recording_id, generation, chunk_index, start_ms, end_ms, text,
              segments_json, language, completed_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                recording_id,
                generation,
                chunk.index,
                chunk.start_ms.min(i64::MAX as u64) as i64,
                chunk.end_ms.min(i64::MAX as u64) as i64,
                &chunk.text,
                segments_json,
                chunk.language.as_deref(),
                Utc::now().to_rfc3339()
            ],
        )?;
        transaction.execute(
            "UPDATE transcriptions
             SET completed_chunks = (
               SELECT COUNT(*) FROM transcription_chunks
               WHERE recording_id = ?1 AND generation = ?2
             ), updated_at = ?3
             WHERE recording_id = ?1 AND generation = ?2",
            params![recording_id, generation, Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn completed_transcription_chunks(
        &self,
        recording_id: &str,
        generation: u32,
    ) -> Result<Vec<u32>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT chunk_index FROM transcription_chunks
             WHERE recording_id = ?1 AND generation = ?2 ORDER BY chunk_index",
        )?;
        let rows = statement.query_map(params![recording_id, generation], |row| {
            row.get::<_, i64>(0)
                .map(|value| value.max(0).min(u32::MAX as i64) as u32)
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn complete_transcription(
        &self,
        recording_id: &str,
        generation: u32,
        text: &str,
        segments: &[TranscriptSegment],
        language: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET status = 'completed', text = ?3, segments_json = ?4, language = ?5,
                 completed_chunks = total_chunks, error_message = NULL,
                 updated_at = ?6, completed_at = ?6
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                text,
                serde_json::to_string(segments)?,
                language,
                now
            ],
        )?;
        Ok(())
    }

    pub fn set_transcription_error(
        &self,
        recording_id: &str,
        generation: u32,
        status: TranscriptionStatus,
        message: &str,
    ) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE transcriptions SET status = ?3, error_message = ?4, updated_at = ?5
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                status.as_str(),
                message,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn transcription_summary(&self, recording_id: &str) -> Result<TranscriptionSummary> {
        self.connection
            .lock()
            .query_row(
                "SELECT status, completed_chunks, total_chunks, provider_name, model_id,
                        error_message, text
                 FROM transcriptions WHERE recording_id = ?1",
                [recording_id],
                transcription_summary_from_row,
            )
            .context("该录音还没有转写任务")
    }

    pub fn transcription_generation(&self, recording_id: &str) -> Result<u32> {
        let generation = self.connection.lock().query_row(
            "SELECT generation FROM transcriptions WHERE recording_id = ?1",
            [recording_id],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(generation.max(1).min(u32::MAX as i64) as u32)
    }

    pub fn transcription_provider_snapshot(
        &self,
        recording_id: &str,
    ) -> Result<(String, String, String)> {
        self.connection
            .lock()
            .query_row(
                "SELECT provider_id, provider_name, model_id
                 FROM transcriptions WHERE recording_id = ?1",
                [recording_id],
                |row| {
                    let provider_id = row
                        .get::<_, Option<String>>(0)?
                        .filter(|value| !value.trim().is_empty())
                        .ok_or(rusqlite::Error::InvalidQuery)?;
                    Ok((provider_id, row.get(1)?, row.get(2)?))
                },
            )
            .context("转写任务关联的服务配置已不存在")
    }

    pub fn transcript(&self, recording_id: &str) -> Result<TranscriptDocument> {
        self.connection
            .lock()
            .query_row(
                "SELECT status, provider_name, model_id, language, text, segments_json,
                        completed_chunks, total_chunks, error_message, updated_at
                 FROM transcriptions WHERE recording_id = ?1",
                [recording_id],
                |row| {
                    let segments_json: String = row.get(5)?;
                    Ok(TranscriptDocument {
                        recording_id: recording_id.to_owned(),
                        status: TranscriptionStatus::from_str(&row.get::<_, String>(0)?),
                        provider_name: row.get(1)?,
                        model_id: row.get(2)?,
                        language: row.get(3)?,
                        text: row.get(4)?,
                        segments: serde_json::from_str(&segments_json).unwrap_or_default(),
                        completed_chunks: row.get::<_, i64>(6)?.max(0) as u32,
                        total_chunks: row.get::<_, i64>(7)?.max(0) as u32,
                        error_message: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .context("该录音还没有转写结果")
    }

    pub fn transcription_chunks(
        &self,
        recording_id: &str,
        generation: u32,
    ) -> Result<Vec<StoredTranscriptionChunk>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT chunk_index, start_ms, end_ms, text, segments_json, language
             FROM transcription_chunks
             WHERE recording_id = ?1 AND generation = ?2
             ORDER BY chunk_index",
        )?;
        let rows = statement.query_map(params![recording_id, generation], |row| {
            let json: String = row.get(4)?;
            Ok(StoredTranscriptionChunk {
                index: row.get::<_, i64>(0)?.max(0) as u32,
                start_ms: row.get::<_, i64>(1)?.max(0) as u64,
                end_ms: row.get::<_, i64>(2)?.max(0) as u64,
                text: row.get(3)?,
                segments: serde_json::from_str(&json).unwrap_or_default(),
                language: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn interrupt_running_transcriptions(&self) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET status = 'interrupted', error_message = 'Nota 上次退出时转写尚未完成',
                 updated_at = ?1
             WHERE status IN ('queued', 'preparing', 'transcribing')",
            [Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

fn recording_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordingItem> {
    let status: Option<String> = row.get(7)?;
    let transcription = if let Some(status) = status {
        let completed_chunks = row.get::<_, Option<i64>>(8)?.unwrap_or(0).max(0) as u32;
        let total_chunks = row.get::<_, Option<i64>>(9)?.unwrap_or(0).max(0) as u32;
        let provider_name = row.get::<_, Option<String>>(10)?.unwrap_or_default();
        let model_id = row.get::<_, Option<String>>(11)?.unwrap_or_default();
        let error_message = row.get(12)?;
        let text = row.get::<_, Option<String>>(13)?.unwrap_or_default();
        Some(TranscriptionSummary {
            status: TranscriptionStatus::from_str(&status),
            completed_chunks,
            total_chunks,
            provider_name,
            model_id,
            error_message,
            has_text: !text.trim().is_empty(),
        })
    } else {
        None
    };
    Ok(RecordingItem {
        id: row.get(0)?,
        title: row.get(1)?,
        path: row.get(2)?,
        created_at: row.get(3)?,
        duration_ms: row.get::<_, i64>(4)?.max(0) as u64,
        size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
        recovered: row.get::<_, i32>(6)? != 0,
        transcription,
    })
}

fn provider_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AsrProvider> {
    Ok(AsrProvider {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: AsrProviderKind::from_str(&row.get::<_, String>(2)?),
        base_url: row.get(3)?,
        model_id: row.get(4)?,
        has_api_key: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn transcription_summary_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<TranscriptionSummary> {
    let text: String = row.get(6)?;
    Ok(TranscriptionSummary {
        status: TranscriptionStatus::from_str(&row.get::<_, String>(0)?),
        completed_chunks: row.get::<_, i64>(1)?.max(0) as u32,
        total_chunks: row.get::<_, i64>(2)?.max(0) as u32,
        provider_name: row.get(3)?,
        model_id: row.get(4)?,
        error_message: row.get(5)?,
        has_text: !text.trim().is_empty(),
    })
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

    fn test_storage() -> (PathBuf, Storage) {
        let root = std::env::temp_dir().join(format!("nota-storage-test-{}", uuid::Uuid::new_v4()));
        let nota = root.join("Nota");
        let recordings = nota.join("Recordings");
        let recovery = nota.join("Recovery");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::create_dir_all(&recovery).unwrap();
        let storage = Storage::open(AppPaths {
            recovery,
            logs: nota.join("Logs"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database: nota.join("nota.db"),
        })
        .unwrap();
        (root, storage)
    }

    fn provider_request(id: Option<String>, api_key: AsrApiKeyUpdate) -> SaveAsrProviderRequest {
        SaveAsrProviderRequest {
            id,
            name: "LAN FunASR".into(),
            kind: AsrProviderKind::FunAsr,
            base_url: "http://127.0.0.1:8000/v1".into(),
            model_id: "sensevoice".into(),
            api_key,
        }
    }

    fn probe_request(id: Option<String>, api_key: AsrApiKeyUpdate) -> AsrProviderProbeRequest {
        AsrProviderProbeRequest {
            id,
            kind: AsrProviderKind::OpenAiCompatible,
            base_url: "http://draft.example.test/v1".into(),
            model_id: "draft-model".into(),
            api_key,
        }
    }

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

    #[test]
    fn opens_a_pre_asr_database_and_applies_asr_schema_defaults() {
        let root =
            std::env::temp_dir().join(format!("nota-storage-migration-{}", uuid::Uuid::new_v4()));
        let nota = root.join("Nota");
        let recordings = nota.join("Recordings");
        let recovery = nota.join("Recovery");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::create_dir_all(&recovery).unwrap();
        let database = nota.join("nota.db");
        {
            let connection = Connection::open(&database).unwrap();
            migration_schema(&connection);
            connection
                .execute(
                    "INSERT INTO settings(key, value) VALUES('microphone_enabled', 'true')",
                    [],
                )
                .unwrap();
        }

        let storage = Storage::open(AppPaths {
            recovery,
            logs: nota.join("Logs"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database,
        })
        .unwrap();
        let settings = storage.settings().unwrap();
        assert_eq!(settings.active_asr_provider_id, None);
        assert!(!settings.auto_transcribe);
        assert!(storage.list_asr_providers().unwrap().is_empty());
        let table_count: i64 = storage
            .connection
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN
                   ('asr_providers', 'transcriptions', 'transcription_chunks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 3);
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn api_key_updates_are_masked_and_support_keep_replace_and_clear() {
        let (root, storage) = test_storage();
        let provider = storage
            .save_asr_provider(provider_request(
                None,
                AsrApiKeyUpdate::Replace {
                    value: "first-secret".into(),
                },
            ))
            .unwrap();
        assert!(provider.has_api_key);
        assert!(storage.list_asr_providers().unwrap()[0].has_api_key);
        assert_eq!(
            storage.find_asr_provider(&provider.id).unwrap().api_key,
            "first-secret"
        );

        storage
            .save_asr_provider(provider_request(
                Some(provider.id.clone()),
                AsrApiKeyUpdate::Keep,
            ))
            .unwrap();
        assert_eq!(
            storage.find_asr_provider(&provider.id).unwrap().api_key,
            "first-secret"
        );
        storage
            .save_asr_provider(provider_request(
                Some(provider.id.clone()),
                AsrApiKeyUpdate::Replace {
                    value: "second-secret".into(),
                },
            ))
            .unwrap();
        assert_eq!(
            storage.find_asr_provider(&provider.id).unwrap().api_key,
            "second-secret"
        );
        let cleared = storage
            .save_asr_provider(provider_request(
                Some(provider.id.clone()),
                AsrApiKeyUpdate::Clear,
            ))
            .unwrap();
        assert!(!cleared.has_api_key);
        assert!(
            storage
                .find_asr_provider(&provider.id)
                .unwrap()
                .api_key
                .is_empty()
        );
        let secure_delete: i64 = storage
            .connection
            .lock()
            .query_row("PRAGMA secure_delete", [], |row| row.get(0))
            .unwrap();
        assert_eq!(secure_delete, 1);
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn probe_credentials_use_draft_values_without_persisting_them() {
        let (root, storage) = test_storage();
        let provider = storage
            .save_asr_provider(provider_request(
                None,
                AsrApiKeyUpdate::Replace {
                    value: "saved-secret".into(),
                },
            ))
            .unwrap();

        let kept = storage
            .asr_probe_credentials(probe_request(
                Some(provider.id.clone()),
                AsrApiKeyUpdate::Keep,
            ))
            .unwrap();
        assert_eq!(kept.api_key, "saved-secret");
        assert_eq!(kept.provider.base_url, "http://draft.example.test/v1");
        assert_eq!(kept.provider.model_id, "draft-model");
        assert_eq!(kept.provider.kind, AsrProviderKind::OpenAiCompatible);

        let replaced = storage
            .asr_probe_credentials(probe_request(
                Some(provider.id.clone()),
                AsrApiKeyUpdate::Replace {
                    value: "temporary-secret".into(),
                },
            ))
            .unwrap();
        assert_eq!(replaced.api_key, "temporary-secret");
        let cleared = storage
            .asr_probe_credentials(probe_request(
                Some(provider.id.clone()),
                AsrApiKeyUpdate::Clear,
            ))
            .unwrap();
        assert!(cleared.api_key.is_empty());
        let unsaved = storage
            .asr_probe_credentials(probe_request(None, AsrApiKeyUpdate::Keep))
            .unwrap();
        assert!(unsaved.api_key.is_empty());

        let persisted = storage.find_asr_provider(&provider.id).unwrap();
        assert_eq!(persisted.api_key, "saved-secret");
        assert_eq!(persisted.provider.base_url, provider.base_url);
        assert_eq!(persisted.provider.model_id, provider.model_id);
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_provider_clears_default_and_auto_transcribe() {
        let (root, storage) = test_storage();
        let provider = storage
            .save_asr_provider(provider_request(None, AsrApiKeyUpdate::Keep))
            .unwrap();
        let mut settings = storage.settings().unwrap();
        settings.active_asr_provider_id = Some(provider.id.clone());
        settings.auto_transcribe = true;
        storage.save_settings(&settings).unwrap();

        storage.delete_asr_provider(&provider.id).unwrap();

        let settings = storage.settings().unwrap();
        assert_eq!(settings.active_asr_provider_id, None);
        assert!(!settings.auto_transcribe);
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_recording_cascades_transcription_chunks() {
        let (root, storage) = test_storage();
        let recording_path = storage.paths.default_recordings.join("meeting.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        let recording = RecordingItem {
            id: "recording".into(),
            title: "meeting".into(),
            path: recording_path.to_string_lossy().into_owned(),
            created_at: "2026-07-28T00:00:00Z".into(),
            duration_ms: 10_000,
            size_bytes: 5,
            recovered: false,
            transcription: None,
        };
        storage.insert_recording(&recording).unwrap();
        let provider = storage
            .save_asr_provider(provider_request(None, AsrApiKeyUpdate::Keep))
            .unwrap();
        let generation = storage
            .begin_transcription(&recording.id, &provider)
            .unwrap();
        storage
            .save_transcription_chunk(
                &recording.id,
                generation,
                &StoredTranscriptionChunk {
                    index: 0,
                    start_ms: 0,
                    end_ms: 10_000,
                    text: "test".into(),
                    segments: Vec::new(),
                    language: Some("en".into()),
                },
            )
            .unwrap();

        storage.remove_recording(&recording.id).unwrap();

        let chunk_count: i64 = storage
            .connection
            .lock()
            .query_row("SELECT COUNT(*) FROM transcription_chunks", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(chunk_count, 0);
        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }
}
