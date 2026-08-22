use crate::models::{
    AecMode, AiDocument, AiDocumentVersion, AiFileState, AiGenerationDetails, AiGenerationMode,
    AiGenerationStatus, AiMeetingProfile, AiTemplate, AiWorkspace, AppSettings, AsrApiKeyUpdate,
    AsrProvider, AsrProviderCredentials, AsrProviderKind, AsrProviderProbeRequest, LlmProvider,
    LlmProviderCredentials, LlmProviderKind, LlmProviderProbeRequest, ParticipantProfile,
    RecordingItem, RecordingOrigin, RecordingSpeakerAssignment, SaveAiTemplateRequest,
    SaveAsrProviderRequest, SaveLlmProviderRequest, SpeakerIdentificationAssignment,
    StoredTranscriptionChunk, TranscriptDocument, TranscriptSegment, TranscriptionExecution,
    TranscriptionProgressPhase, TranscriptionProgressUnit, TranscriptionProtocol,
    TranscriptionStatus, TranscriptionSummary, VoiceprintSample,
};
use crate::paths::AppPaths;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct StoredVoiceprintEmbedding {
    pub participant_id: String,
    pub display_name: String,
    pub embedding_fingerprint: String,
    pub dimension: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct VoiceprintEnrollment {
    pub raw_speaker: String,
    pub participant_id: Option<String>,
    pub new_display_name: Option<String>,
    pub match_score: Option<f32>,
    pub embedding_fingerprint: String,
    pub embedding: Option<Vec<f32>>,
    pub preview_start_ms: u64,
    pub preview_end_ms: u64,
    pub speech_duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AudioImportJob {
    pub id: String,
    pub source_path: String,
    pub source_file_name: String,
    pub source_format: Option<String>,
    pub source_sha256: Option<String>,
    pub title: String,
    pub created_at: String,
    pub imported_at: String,
    pub final_path: String,
    pub partial_path: String,
    pub duration_ms: Option<u64>,
    pub size_bytes: Option<u64>,
    pub status: String,
}

pub struct NewAiVersion<'a> {
    pub id: &'a str,
    pub document_id: &'a str,
    pub version_number: u32,
    pub mode: AiGenerationMode,
    pub parent_version_id: Option<&'a str>,
    pub file_path: &'a str,
    pub provider_id: &'a str,
    pub provider: &'a LlmProvider,
    pub template: &'a AiTemplate,
    pub transcription_generation: u32,
    pub speaker_names_json: &'a str,
    pub meeting_context: &'a str,
    pub document_requirements: &'a str,
    pub run_request: &'a str,
    pub estimated_input_tokens: u32,
    pub request_body_json: &'a str,
}

fn resolve_assignment_participant(
    transaction: &Transaction<'_>,
    participant_id: Option<&str>,
    new_display_name: Option<&str>,
    now: &str,
) -> Result<Option<String>> {
    let participant_id = participant_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let new_display_name = new_display_name
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if participant_id.is_some() && new_display_name.is_some() {
        bail!("不能同时选择现有说话人并新建姓名");
    }
    if let Some(id) = participant_id {
        let exists = transaction
            .query_row("SELECT 1 FROM participants WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            bail!("选择的说话人已经不存在");
        }
        return Ok(Some(id.to_owned()));
    }
    let Some(name) = new_display_name else {
        return Ok(None);
    };
    if name.chars().count() > 80 {
        bail!("说话人姓名不能超过 80 个字符");
    }
    if let Some(existing) = transaction
        .query_row(
            "SELECT id FROM participants WHERE display_name = ?1 COLLATE NOCASE",
            [name],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(Some(existing));
    }
    let id = uuid::Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO participants(id, display_name, created_at, updated_at)
         VALUES(?1, ?2, ?3, ?3)",
        params![id, name, now],
    )?;
    Ok(Some(id))
}

struct RecordingSpeakerAssignmentUpsert<'a> {
    recording_id: &'a str,
    generation: u32,
    raw_speaker: &'a str,
    participant_id: &'a str,
    match_score: Option<f32>,
    assignment_source: &'a str,
    now: &'a str,
}

fn upsert_recording_speaker_assignment(
    transaction: &Transaction<'_>,
    assignment: RecordingSpeakerAssignmentUpsert<'_>,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO recording_speaker_assignments
         (recording_id, generation, raw_speaker, participant_id,
          match_score, assignment_source, confirmed_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(recording_id, generation, raw_speaker) DO UPDATE SET
           participant_id = excluded.participant_id,
           match_score = excluded.match_score,
           assignment_source = excluded.assignment_source,
           confirmed_at = excluded.confirmed_at",
        params![
            assignment.recording_id,
            assignment.generation,
            assignment.raw_speaker,
            assignment.participant_id,
            assignment.match_score,
            assignment.assignment_source,
            assignment.now,
        ],
    )?;
    Ok(())
}

pub struct Storage {
    connection: Mutex<Connection>,
    paths: AppPaths,
}

struct BuiltinAiTemplate {
    id: &'static str,
    key: &'static str,
    name: &'static str,
    description: &'static str,
    instructions: &'static str,
    requirements: &'static str,
    requires_speaker_labels: bool,
}

const BUILTIN_AI_TEMPLATES: [BuiltinAiTemplate; 4] = [
    BuiltinAiTemplate {
        id: "builtin-meeting-summary",
        key: "meeting_summary",
        name: "会议总结",
        description: "提炼会议主题、结论、风险与未决问题",
        instructions: "总结整场会议。只保留能够从转写或用户上下文中得到支持的信息；明确区分已确认结论与仍待确认事项。",
        requirements: "使用 Markdown，至少包含：会议摘要、主要议题、结论与决策、风险与未决问题。没有内容的章节写“未提及”，不要编造。",
        requires_speaker_labels: false,
    },
    BuiltinAiTemplate {
        id: "builtin-action-items",
        key: "action_items",
        name: "待办清单",
        description: "从会议中提取可执行的后续事项",
        instructions: "提取会议中明确提出或承诺的待办事项。不要把已经完成的事项重新列为待办，也不要推断负责人或截止时间。",
        requirements: "使用 Markdown checkbox。每项包含事项、负责人、截止时间和来源；未明确的信息写“未指定”。没有待办时明确写“未发现明确待办”。",
        requires_speaker_labels: false,
    },
    BuiltinAiTemplate {
        id: "builtin-speaker-summary",
        key: "speaker_summary",
        name: "按发言人总结",
        description: "分别整理每位发言人的观点、结论和承诺",
        instructions: "按转写中的发言人分别总结。保留说话人显示名；没有显示名时使用原始 speaker_N。只总结其表达的要点、结论和承诺，不评价个人表现。",
        requirements: "使用每位发言人一个二级标题，下面按要点、结论、承诺组织；缺失内容写“未提及”。",
        requires_speaker_labels: true,
    },
    BuiltinAiTemplate {
        id: "builtin-speaker-standup",
        key: "speaker_standup",
        name: "按发言人待办",
        description: "按发言人生成晨会完成事项、今日待办和阻塞项",
        instructions: "按发言人整理晨会信息。不得把昨日已经完成的事项归入今日待办，不得把一人的事项归给另一人，也不得推断未明确表达的负责人。",
        requirements: "每位发言人一个二级标题，并固定包含“昨日完成”“今日待办”“阻塞项”。今日待办使用 Markdown checkbox；未出现的信息写“未提及”。",
        requires_speaker_labels: true,
    },
];

fn seed_ai_templates(connection: &Connection) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    for template in BUILTIN_AI_TEMPLATES {
        connection.execute(
            "INSERT INTO ai_templates
             (id, name, description, builtin_key, task_instructions,
              output_requirements, requires_speaker_labels, revision, archived, created_at, updated_at)
              VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 0, ?8, ?8)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               description = excluded.description,
               builtin_key = excluded.builtin_key,
               task_instructions = excluded.task_instructions,
                output_requirements = excluded.output_requirements,
                requires_speaker_labels = excluded.requires_speaker_labels,
                archived = 0,
               updated_at = excluded.updated_at",
            params![
                template.id,
                template.name,
                template.description,
                template.key,
                template.instructions,
                template.requirements,
                template.requires_speaker_labels,
                now,
            ],
        )?;
    }
    Ok(())
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
              recovered INTEGER NOT NULL DEFAULT 0,
              origin TEXT NOT NULL DEFAULT 'captured',
              source_file_name TEXT,
              source_format TEXT,
              source_sha256 TEXT,
              imported_at TEXT
            );
            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              session_id TEXT NOT NULL,
              component TEXT NOT NULL,
              code TEXT NOT NULL,
              occurred_at TEXT NOT NULL,
              detail TEXT NOT NULL
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
              provider_kind TEXT NOT NULL DEFAULT 'open_ai_compatible',
              model_id TEXT NOT NULL,
              speaker_count INTEGER CHECK(speaker_count BETWEEN 1 AND 100),
              status TEXT NOT NULL,
              completed_chunks INTEGER NOT NULL DEFAULT 0,
              total_chunks INTEGER NOT NULL DEFAULT 0,
              text TEXT NOT NULL DEFAULT '',
              segments_json TEXT NOT NULL DEFAULT '[]',
              language TEXT,
              error_message TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              completed_at TEXT,
              protocol TEXT NOT NULL DEFAULT 'legacy_chunks',
              remote_job_id TEXT,
              provider_state_json TEXT NOT NULL DEFAULT '{}',
              idempotency_key TEXT NOT NULL DEFAULT '',
              progress_phase TEXT,
              progress_current INTEGER NOT NULL DEFAULT 0,
              progress_total INTEGER NOT NULL DEFAULT 0,
              progress_unit TEXT
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
            CREATE TABLE IF NOT EXISTS participants (
              id TEXT PRIMARY KEY,
              display_name TEXT NOT NULL COLLATE NOCASE UNIQUE,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS voiceprints (
              id TEXT PRIMARY KEY,
              participant_id TEXT NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
              embedding_model TEXT NOT NULL,
              embedding_fingerprint TEXT NOT NULL,
              dimension INTEGER NOT NULL,
              embedding BLOB NOT NULL,
              source_recording_id TEXT REFERENCES recordings(id) ON DELETE SET NULL,
              source_generation INTEGER NOT NULL,
              source_speaker TEXT NOT NULL,
              preview_start_ms INTEGER NOT NULL,
              preview_end_ms INTEGER NOT NULL,
              speech_duration_ms INTEGER NOT NULL,
              created_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS voiceprints_source_unique
              ON voiceprints(source_recording_id, source_generation, source_speaker)
              WHERE source_recording_id IS NOT NULL;
            CREATE TABLE IF NOT EXISTS recording_speaker_assignments (
              recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
              generation INTEGER NOT NULL,
              raw_speaker TEXT NOT NULL,
              participant_id TEXT NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
              match_score REAL,
              assignment_source TEXT NOT NULL,
              confirmed_at TEXT NOT NULL,
              PRIMARY KEY(recording_id, generation, raw_speaker)
            );
            CREATE TABLE IF NOT EXISTS llm_providers (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              kind TEXT NOT NULL,
              base_url TEXT NOT NULL,
              api_key TEXT NOT NULL DEFAULT '',
              model_id TEXT NOT NULL,
              input_token_budget INTEGER NOT NULL DEFAULT 32768,
              max_output_tokens INTEGER NOT NULL DEFAULT 4096,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ai_templates (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              description TEXT NOT NULL DEFAULT '',
              builtin_key TEXT,
              task_instructions TEXT NOT NULL,
              output_requirements TEXT NOT NULL,
              requires_speaker_labels INTEGER NOT NULL DEFAULT 0,
              revision INTEGER NOT NULL DEFAULT 1,
              archived INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS ai_templates_builtin_unique
              ON ai_templates(builtin_key) WHERE builtin_key IS NOT NULL;
            CREATE TABLE IF NOT EXISTS ai_meeting_profiles (
              recording_id TEXT PRIMARY KEY REFERENCES recordings(id) ON DELETE CASCADE,
              workspace_path TEXT NOT NULL,
              meeting_context TEXT NOT NULL DEFAULT '',
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ai_documents (
              id TEXT PRIMARY KEY,
              recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
              template_id TEXT NOT NULL REFERENCES ai_templates(id),
              title TEXT NOT NULL,
              requirements TEXT NOT NULL DEFAULT '',
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              UNIQUE(recording_id, template_id)
            );
            CREATE TABLE IF NOT EXISTS ai_document_versions (
              id TEXT PRIMARY KEY,
              document_id TEXT NOT NULL REFERENCES ai_documents(id) ON DELETE CASCADE,
              version_number INTEGER NOT NULL,
              mode TEXT NOT NULL,
              parent_version_id TEXT REFERENCES ai_document_versions(id) ON DELETE SET NULL,
              status TEXT NOT NULL,
              file_path TEXT,
              generated_hash TEXT,
              provider_id TEXT REFERENCES llm_providers(id) ON DELETE SET NULL,
              provider_name TEXT NOT NULL,
              provider_kind TEXT NOT NULL,
              model_id TEXT NOT NULL,
              template_name TEXT NOT NULL,
              template_revision INTEGER NOT NULL,
              template_instructions TEXT NOT NULL,
              template_output_requirements TEXT NOT NULL,
              system_policy_version INTEGER NOT NULL DEFAULT 1,
              transcription_generation INTEGER NOT NULL,
              speaker_names_json TEXT NOT NULL DEFAULT '{}',
              meeting_context TEXT NOT NULL DEFAULT '',
              document_requirements TEXT NOT NULL DEFAULT '',
              run_request TEXT NOT NULL DEFAULT '',
              estimated_input_tokens INTEGER NOT NULL DEFAULT 0,
              input_tokens INTEGER,
              output_tokens INTEGER,
              request_body_json TEXT,
              response_body_json TEXT,
              error_message TEXT,
              created_at TEXT NOT NULL,
              completed_at TEXT,
              UNIQUE(document_id, version_number)
            );
            ",
        )?;
        ensure_recording_import_columns(&connection)?;
        ensure_audio_import_schema(&connection)?;
        ensure_ai_template_columns(&connection)?;
        ensure_ai_generation_detail_columns(&connection)?;
        seed_ai_templates(&connection)?;
        ensure_transcription_job_columns(&connection)?;
        ensure_transcription_speaker_count_constraint(&connection)?;
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
        let ai_documents_directory = self
            .setting_value(&connection, "ai_documents_directory")?
            .unwrap_or_else(|| {
                self.paths
                    .default_ai_documents
                    .to_string_lossy()
                    .into_owned()
            });
        let aec_mode = match self.setting_value(&connection, "aec_mode")?.as_deref() {
            Some("on") => AecMode::On,
            Some("off") => AecMode::Off,
            _ => AecMode::Auto,
        };
        let microphone_enabled = self
            .setting_value(&connection, "microphone_enabled")?
            .map(|value| value == "true")
            .unwrap_or(true);
        let first_run_complete = self
            .setting_value(&connection, "first_run_complete")?
            .map(|value| value == "true")
            .unwrap_or(false);
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
        let voiceprint_provider_id = self
            .setting_value(&connection, "voiceprint_provider_id")?
            .filter(|value| !value.trim().is_empty());
        let auto_transcribe = self
            .setting_value(&connection, "auto_transcribe")?
            .map(|value| value == "true")
            .unwrap_or(false);
        let active_llm_provider_id = self
            .setting_value(&connection, "active_llm_provider_id")?
            .filter(|value| !value.trim().is_empty());
        Ok(AppSettings {
            output_directory,
            ai_documents_directory,
            aec_mode,
            microphone_enabled,
            first_run_complete,
            shortcuts_enabled,
            toggle_shortcut,
            stop_shortcut,
            active_asr_provider_id,
            voiceprint_provider_id,
            auto_transcribe,
            active_llm_provider_id,
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
                "ai_documents_directory",
                settings.ai_documents_directory.clone(),
            ),
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
            (
                "first_run_complete",
                settings.first_run_complete.to_string(),
            ),
            ("shortcuts_enabled", settings.shortcuts_enabled.to_string()),
            ("toggle_shortcut", settings.toggle_shortcut.clone()),
            ("stop_shortcut", settings.stop_shortcut.clone()),
            (
                "active_asr_provider_id",
                settings.active_asr_provider_id.clone().unwrap_or_default(),
            ),
            (
                "voiceprint_provider_id",
                settings.voiceprint_provider_id.clone().unwrap_or_default(),
            ),
            ("auto_transcribe", settings.auto_transcribe.to_string()),
            (
                "active_llm_provider_id",
                settings.active_llm_provider_id.clone().unwrap_or_default(),
            ),
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

    pub fn begin_audio_import(&self, job: &AudioImportJob) -> Result<()> {
        self.connection.lock().execute(
            "INSERT INTO audio_import_jobs
             (id, source_path, source_file_name, source_format, source_sha256,
              title, created_at, imported_at, final_path, partial_path,
              duration_ms, size_bytes, status, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                job.id,
                job.source_path,
                job.source_file_name,
                job.source_format,
                job.source_sha256,
                job.title,
                job.created_at,
                job.imported_at,
                job.final_path,
                job.partial_path,
                job.duration_ms
                    .map(|value| value.min(i64::MAX as u64) as i64),
                job.size_bytes
                    .map(|value| value.min(i64::MAX as u64) as i64),
                job.status,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn prepare_audio_import(
        &self,
        id: &str,
        source_format: &str,
        source_sha256: &str,
        duration_ms: u64,
        size_bytes: u64,
    ) -> Result<()> {
        let changed = self.connection.lock().execute(
            "UPDATE audio_import_jobs
             SET source_format = ?1, source_sha256 = ?2, duration_ms = ?3,
                 size_bytes = ?4, status = 'prepared', updated_at = ?5
             WHERE id = ?6",
            params![
                source_format,
                source_sha256,
                duration_ms.min(i64::MAX as u64) as i64,
                size_bytes.min(i64::MAX as u64) as i64,
                Utc::now().to_rfc3339(),
                id,
            ],
        )?;
        if changed == 0 {
            bail!("找不到音频导入任务");
        }
        Ok(())
    }

    pub fn mark_audio_import_file_committed(&self, id: &str) -> Result<()> {
        let changed = self.connection.lock().execute(
            "UPDATE audio_import_jobs
             SET status = 'file_committed', updated_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        if changed == 0 {
            bail!("找不到音频导入任务");
        }
        Ok(())
    }

    pub fn complete_audio_import(&self, id: &str) -> Result<RecordingItem> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let job = transaction
            .query_row(
                "SELECT id, source_path, source_file_name, source_format, source_sha256,
                        title, created_at, imported_at, final_path, partial_path,
                        duration_ms, size_bytes, status
                 FROM audio_import_jobs WHERE id = ?1",
                [id],
                audio_import_job_from_row,
            )
            .context("找不到音频导入任务")?;
        let source_format = job.source_format.context("导入任务缺少音频格式")?;
        let source_sha256 = job.source_sha256.context("导入任务缺少文件摘要")?;
        let duration_ms = job.duration_ms.context("导入任务缺少音频时长")?;
        let size_bytes = job.size_bytes.context("导入任务缺少文件大小")?;
        transaction.execute(
            "INSERT INTO recordings
             (id, title, path, created_at, duration_ms, size_bytes, recovered,
              origin, source_file_name, source_format, source_sha256, imported_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, 'imported', ?7, ?8, ?9, ?10)",
            params![
                job.id,
                job.title,
                job.final_path,
                job.created_at,
                duration_ms.min(i64::MAX as u64) as i64,
                size_bytes.min(i64::MAX as u64) as i64,
                job.source_file_name,
                source_format,
                source_sha256,
                job.imported_at,
            ],
        )?;
        transaction.execute("DELETE FROM audio_import_jobs WHERE id = ?1", [id])?;
        transaction.commit()?;
        drop(connection);
        self.find_recording(id)
    }

    pub fn audio_import_jobs(&self) -> Result<Vec<AudioImportJob>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, source_path, source_file_name, source_format, source_sha256,
                    title, created_at, imported_at, final_path, partial_path,
                    duration_ms, size_bytes, status
             FROM audio_import_jobs ORDER BY updated_at, id",
        )?;
        let rows = statement.query_map([], audio_import_job_from_row)?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    pub fn remove_audio_import_job(&self, id: &str) -> Result<()> {
        self.connection
            .lock()
            .execute("DELETE FROM audio_import_jobs WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn imported_recording_by_hash(&self, source_sha256: &str) -> Result<Option<RecordingItem>> {
        let id = self
            .connection
            .lock()
            .query_row(
                "SELECT id FROM recordings
                 WHERE origin = 'imported' AND source_sha256 = ?1",
                [source_sha256],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        id.map(|value| self.find_recording(&value)).transpose()
    }

    pub fn list_recordings(&self) -> Result<Vec<RecordingItem>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT r.id, r.title, r.path, r.created_at, r.duration_ms, r.size_bytes, r.recovered,
                     t.status, t.completed_chunks, t.total_chunks, t.provider_name, t.model_id,
                     t.error_message, t.text, t.protocol, t.progress_phase,
                     t.progress_current, t.progress_total, t.progress_unit,
                     t.speaker_count, t.provider_kind, r.origin, r.source_file_name,
                     r.source_format, r.imported_at
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
                         t.error_message, t.text, t.protocol, t.progress_phase,
                         t.progress_current, t.progress_total, t.progress_unit,
                         t.speaker_count, t.provider_kind, r.origin, r.source_file_name,
                         r.source_format, r.imported_at
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
                    let kind = AsrProviderKind::from_str(&row.get::<_, String>(2)?);
                    Ok(AsrProviderCredentials {
                        provider: AsrProvider {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            kind,
                            base_url: row.get(3)?,
                            model_id: row.get(4)?,
                            has_api_key: !api_key.is_empty(),
                            created_at: row.get(6)?,
                            updated_at: row.get(7)?,
                            capabilities: kind.capabilities(),
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
        let kind = request.kind;
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
                kind,
                base_url: request.base_url,
                model_id: request.model_id,
                has_api_key: !api_key.is_empty(),
                created_at: existing
                    .as_ref()
                    .map(|credentials| credentials.provider.created_at.clone())
                    .unwrap_or_else(|| now.clone()),
                updated_at: now,
                capabilities: kind.capabilities(),
            },
            api_key,
        })
    }

    pub fn save_asr_provider(&self, request: SaveAsrProviderRequest) -> Result<AsrProvider> {
        let now = Utc::now().to_rfc3339();
        let voiceprint_compatible = request.kind == AsrProviderKind::FunAsr;
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
        if !voiceprint_compatible {
            transaction.execute(
                "UPDATE settings SET value = ''
                 WHERE key = 'voiceprint_provider_id' AND value = ?1",
                [&id],
            )?;
        }
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
            "UPDATE settings SET value = '' WHERE key = 'voiceprint_provider_id' AND value = ?1",
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

    pub fn list_llm_providers(&self) -> Result<Vec<LlmProvider>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, kind, base_url, model_id, input_token_budget,
                    max_output_tokens, api_key != '', created_at, updated_at
             FROM llm_providers ORDER BY name COLLATE NOCASE, created_at",
        )?;
        let rows = statement.query_map([], llm_provider_from_row)?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    pub fn find_llm_provider(&self, id: &str) -> Result<LlmProviderCredentials> {
        self.connection
            .lock()
            .query_row(
                "SELECT id, name, kind, base_url, model_id, input_token_budget,
                        max_output_tokens, api_key, created_at, updated_at
                 FROM llm_providers WHERE id = ?1",
                [id],
                |row| {
                    let api_key: String = row.get(7)?;
                    Ok(LlmProviderCredentials {
                        provider: LlmProvider {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            kind: LlmProviderKind::from_str(&row.get::<_, String>(2)?),
                            base_url: row.get(3)?,
                            model_id: row.get(4)?,
                            input_token_budget: row.get::<_, i64>(5)?.max(0) as u32,
                            max_output_tokens: row.get::<_, i64>(6)?.max(0) as u32,
                            has_api_key: !api_key.is_empty(),
                            created_at: row.get(8)?,
                            updated_at: row.get(9)?,
                        },
                        api_key,
                    })
                },
            )
            .context("找不到该 AI 模型服务")
    }

    pub fn llm_probe_credentials(
        &self,
        request: LlmProviderProbeRequest,
    ) -> Result<LlmProviderCredentials> {
        let existing = request
            .id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|id| self.find_llm_provider(id))
            .transpose()?;
        let api_key = match request.api_key {
            AsrApiKeyUpdate::Keep => existing
                .as_ref()
                .map(|credentials| credentials.api_key.clone())
                .unwrap_or_default(),
            AsrApiKeyUpdate::Replace { value } => value.trim().to_owned(),
            AsrApiKeyUpdate::Clear => String::new(),
        };
        let now = Utc::now().to_rfc3339();
        Ok(LlmProviderCredentials {
            provider: LlmProvider {
                id: existing
                    .as_ref()
                    .map(|credentials| credentials.provider.id.clone())
                    .unwrap_or_default(),
                name: existing
                    .as_ref()
                    .map(|credentials| credentials.provider.name.clone())
                    .unwrap_or_else(|| "未保存的 AI 模型服务".into()),
                kind: request.kind,
                base_url: request.base_url,
                model_id: request.model_id,
                input_token_budget: request.input_token_budget,
                max_output_tokens: request.max_output_tokens,
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

    pub fn save_llm_provider(&self, request: SaveLlmProviderRequest) -> Result<LlmProvider> {
        validate_llm_provider(&request)?;
        let now = Utc::now().to_rfc3339();
        let id = request
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let connection = self.connection.lock();
        let existing = connection
            .query_row(
                "SELECT api_key, created_at FROM llm_providers WHERE id = ?1",
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
        connection.execute(
            "INSERT INTO llm_providers
             (id, name, kind, base_url, api_key, model_id, input_token_budget,
              max_output_tokens, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               kind = excluded.kind,
               base_url = excluded.base_url,
               api_key = excluded.api_key,
               model_id = excluded.model_id,
               input_token_budget = excluded.input_token_budget,
               max_output_tokens = excluded.max_output_tokens,
               updated_at = excluded.updated_at",
            params![
                id,
                request.name.trim(),
                request.kind.as_str(),
                request.base_url.trim(),
                api_key,
                request.model_id.trim(),
                request.input_token_budget,
                request.max_output_tokens,
                created_at,
                now,
            ],
        )?;
        drop(connection);
        Ok(self.find_llm_provider(&id)?.provider)
    }

    pub fn delete_llm_provider(&self, id: &str) -> Result<()> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let active: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM ai_document_versions
             WHERE provider_id = ?1 AND status IN ('queued', 'generating')",
            [id],
            |row| row.get(0),
        )?;
        if active > 0 {
            bail!("该服务仍有正在排队或生成的 AI 文档，请先取消任务");
        }
        transaction.execute("DELETE FROM llm_providers WHERE id = ?1", [id])?;
        transaction.execute(
            "UPDATE settings SET value = ''
             WHERE key = 'active_llm_provider_id' AND value = ?1",
            [id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_ai_templates(&self) -> Result<Vec<AiTemplate>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, description, builtin_key, task_instructions,
                    output_requirements, requires_speaker_labels, revision, archived, created_at, updated_at
             FROM ai_templates ORDER BY builtin_key IS NULL, name COLLATE NOCASE, created_at",
        )?;
        let rows = statement.query_map([], ai_template_from_row)?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    pub fn find_ai_template(&self, id: &str) -> Result<AiTemplate> {
        self.connection
            .lock()
            .query_row(
                "SELECT id, name, description, builtin_key, task_instructions,
                        output_requirements, requires_speaker_labels, revision, archived, created_at, updated_at
                 FROM ai_templates WHERE id = ?1",
                [id],
                ai_template_from_row,
            )
            .context("找不到该 AI 模板")
    }

    pub fn save_ai_template(&self, request: SaveAiTemplateRequest) -> Result<AiTemplate> {
        validate_ai_template(&request)?;
        let now = Utc::now().to_rfc3339();
        let id = request
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let connection = self.connection.lock();
        let existing = connection
            .query_row(
                "SELECT builtin_key, revision, created_at FROM ai_templates WHERE id = ?1",
                [&id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if existing
            .as_ref()
            .and_then(|value| value.0.as_ref())
            .is_some()
        {
            bail!("内置模板不能直接修改，请先复制为自定义模板");
        }
        let revision = existing.as_ref().map(|value| value.1 + 1).unwrap_or(1);
        let created_at = existing.map(|value| value.2).unwrap_or_else(|| now.clone());
        connection.execute(
            "INSERT INTO ai_templates
             (id, name, description, builtin_key, task_instructions,
              output_requirements, requires_speaker_labels, revision, archived, created_at, updated_at)
             VALUES(?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, 0, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               description = excluded.description,
               task_instructions = excluded.task_instructions,
               output_requirements = excluded.output_requirements,
               requires_speaker_labels = excluded.requires_speaker_labels,
               revision = excluded.revision,
               archived = 0,
               updated_at = excluded.updated_at",
            params![
                id,
                request.name.trim(),
                request.description.trim(),
                request.task_instructions.trim(),
                request.output_requirements.trim(),
                request.requires_speaker_labels,
                revision,
                created_at,
                now,
            ],
        )?;
        drop(connection);
        self.find_ai_template(&id)
    }

    pub fn clone_ai_template(&self, id: &str, name: &str) -> Result<AiTemplate> {
        let source = self.find_ai_template(id)?;
        self.save_ai_template(SaveAiTemplateRequest {
            id: None,
            name: name.to_owned(),
            description: source.description,
            task_instructions: source.task_instructions,
            output_requirements: source.output_requirements,
            requires_speaker_labels: source.requires_speaker_labels,
        })
    }

    pub fn archive_ai_template(&self, id: &str) -> Result<()> {
        let template = self.find_ai_template(id)?;
        if template.builtin_key.is_some() {
            bail!("内置模板不能归档");
        }
        self.connection.lock().execute(
            "UPDATE ai_templates SET archived = 1, updated_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn ai_workspace(&self, recording_id: &str) -> Result<AiWorkspace> {
        let recording = self.find_recording(recording_id)?;
        let stored_profile = {
            let connection = self.connection.lock();
            connection
                .query_row(
                    "SELECT recording_id, workspace_path, meeting_context, updated_at
                 FROM ai_meeting_profiles WHERE recording_id = ?1",
                    [recording_id],
                    ai_meeting_profile_from_row,
                )
                .optional()?
        };
        let current_workspace_path = self
            .default_ai_workspace_path(&recording)
            .to_string_lossy()
            .into_owned();
        let profile = match stored_profile {
            Some(mut profile) => {
                profile.workspace_path = current_workspace_path;
                profile
            }
            None => AiMeetingProfile {
                recording_id: recording_id.to_owned(),
                workspace_path: current_workspace_path,
                meeting_context: String::new(),
                updated_at: String::new(),
            },
        };
        Ok(AiWorkspace {
            profile,
            documents: self.list_ai_documents(recording_id)?,
            templates: self.list_ai_templates()?,
        })
    }

    pub fn save_ai_meeting_profile(
        &self,
        recording_id: &str,
        workspace_path: &str,
        meeting_context: &str,
    ) -> Result<AiMeetingProfile> {
        self.find_recording(recording_id)?;
        let now = Utc::now().to_rfc3339();
        self.connection.lock().execute(
            "INSERT INTO ai_meeting_profiles
             (recording_id, workspace_path, meeting_context, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(recording_id) DO UPDATE SET
               workspace_path = excluded.workspace_path,
               meeting_context = excluded.meeting_context,
               updated_at = excluded.updated_at",
            params![recording_id, workspace_path, meeting_context, now],
        )?;
        Ok(AiMeetingProfile {
            recording_id: recording_id.to_owned(),
            workspace_path: workspace_path.to_owned(),
            meeting_context: meeting_context.to_owned(),
            updated_at: now,
        })
    }

    pub fn create_ai_document(
        &self,
        recording_id: &str,
        template_id: &str,
        title: &str,
        requirements: &str,
    ) -> Result<AiDocument> {
        self.find_recording(recording_id)?;
        self.find_ai_template(template_id)?;
        if self
            .find_ai_document_for_template(recording_id, template_id)?
            .is_some()
        {
            bail!("当前会议已经使用过该模板，请在已有文档中重新生成");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.connection.lock().execute(
            "INSERT INTO ai_documents
             (id, recording_id, template_id, title, requirements, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                recording_id,
                template_id,
                title.trim(),
                requirements,
                now
            ],
        )?;
        self.find_ai_document(&id)
    }

    pub fn update_ai_document(
        &self,
        id: &str,
        title: &str,
        requirements: &str,
    ) -> Result<AiDocument> {
        let changed = self.connection.lock().execute(
            "UPDATE ai_documents
             SET title = ?1, requirements = ?2, updated_at = ?3 WHERE id = ?4",
            params![title.trim(), requirements, Utc::now().to_rfc3339(), id],
        )?;
        if changed == 0 {
            bail!("找不到该 AI 文档");
        }
        self.find_ai_document(id)
    }

    pub fn find_ai_document_for_template(
        &self,
        recording_id: &str,
        template_id: &str,
    ) -> Result<Option<AiDocument>> {
        let id = self
            .connection
            .lock()
            .query_row(
                "SELECT id FROM ai_documents WHERE recording_id = ?1 AND template_id = ?2",
                params![recording_id, template_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        id.map(|id| self.find_ai_document(&id)).transpose()
    }

    pub fn find_ai_document(&self, id: &str) -> Result<AiDocument> {
        let mut document = self
            .connection
            .lock()
            .query_row(
                "SELECT d.id, d.recording_id, d.template_id, d.title, d.requirements,
                        t.name, t.builtin_key, d.created_at, d.updated_at
                 FROM ai_documents d
                 JOIN ai_templates t ON t.id = d.template_id
                 WHERE d.id = ?1",
                [id],
                ai_document_from_row,
            )
            .context("找不到该 AI 文档")?;
        document.latest_version = self.list_ai_document_versions(id)?.into_iter().next();
        Ok(document)
    }

    pub fn list_ai_documents(&self, recording_id: &str) -> Result<Vec<AiDocument>> {
        let documents = {
            let connection = self.connection.lock();
            let mut statement = connection.prepare(
                "SELECT d.id, d.recording_id, d.template_id, d.title, d.requirements,
                        t.name, t.builtin_key, d.created_at, d.updated_at
                 FROM ai_documents d
                 JOIN ai_templates t ON t.id = d.template_id
                 WHERE d.recording_id = ?1 ORDER BY d.created_at",
            )?;
            statement
                .query_map([recording_id], ai_document_from_row)?
                .filter_map(std::result::Result::ok)
                .collect::<Vec<_>>()
        };
        documents
            .into_iter()
            .map(|mut document| {
                document.latest_version = self
                    .list_ai_document_versions(&document.id)?
                    .into_iter()
                    .find(|version| version.status == AiGenerationStatus::Completed);
                Ok(document)
            })
            .collect()
    }

    pub fn next_ai_version_number(&self, document_id: &str) -> Result<u32> {
        let next: i64 = self.connection.lock().query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1
             FROM ai_document_versions WHERE document_id = ?1",
            [document_id],
            |row| row.get(0),
        )?;
        Ok(next.max(1) as u32)
    }

    pub fn insert_ai_document_version(&self, value: NewAiVersion<'_>) -> Result<AiDocumentVersion> {
        let now = Utc::now().to_rfc3339();
        self.connection.lock().execute(
            "INSERT INTO ai_document_versions
             (id, document_id, version_number, mode, parent_version_id, status,
              file_path, provider_id, provider_name, provider_kind, model_id,
              template_name, template_revision, template_instructions,
              template_output_requirements, system_policy_version,
              transcription_generation, speaker_names_json, meeting_context,
              document_requirements, run_request, estimated_input_tokens,
              request_body_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, ?14, 1, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                value.id,
                value.document_id,
                value.version_number,
                value.mode.as_str(),
                value.parent_version_id,
                value.file_path,
                value.provider_id,
                value.provider.name,
                value.provider.kind.as_str(),
                value.provider.model_id,
                value.template.name,
                value.template.revision,
                value.template.task_instructions,
                value.template.output_requirements,
                value.transcription_generation,
                value.speaker_names_json,
                value.meeting_context,
                value.document_requirements,
                value.run_request,
                value.estimated_input_tokens,
                value.request_body_json,
                now,
            ],
        )?;
        self.find_ai_document_version(value.id)
    }

    pub fn set_ai_version_status(
        &self,
        id: &str,
        status: AiGenerationStatus,
        error_message: Option<&str>,
    ) -> Result<AiDocumentVersion> {
        let completed_at = matches!(
            status,
            AiGenerationStatus::Completed
                | AiGenerationStatus::Failed
                | AiGenerationStatus::Cancelled
                | AiGenerationStatus::Interrupted
        )
        .then(|| Utc::now().to_rfc3339());
        self.connection.lock().execute(
            "UPDATE ai_document_versions
             SET status = ?1, error_message = ?2,
                 completed_at = COALESCE(?3, completed_at)
             WHERE id = ?4",
            params![status.as_str(), error_message, completed_at, id],
        )?;
        self.find_ai_document_version(id)
    }

    pub fn complete_ai_document_version(
        &self,
        id: &str,
        generated_hash: &str,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        response_body_json: &str,
    ) -> Result<AiDocumentVersion> {
        let now = Utc::now().to_rfc3339();
        self.connection.lock().execute(
            "UPDATE ai_document_versions
             SET status = 'completed', generated_hash = ?1, input_tokens = ?2,
                 output_tokens = ?3, response_body_json = ?4,
                 error_message = NULL, completed_at = ?5
             WHERE id = ?6",
            params![
                generated_hash,
                input_tokens,
                output_tokens,
                response_body_json,
                now,
                id
            ],
        )?;
        self.find_ai_document_version(id)
    }

    pub fn find_ai_generation_details(&self, version_id: &str) -> Result<AiGenerationDetails> {
        let (request_body_json, response_body_json): (Option<String>, Option<String>) = self
            .connection
            .lock()
            .query_row(
                "SELECT request_body_json, response_body_json
                 FROM ai_document_versions WHERE id = ?1",
                [version_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("找不到该 AI 文档版本")?;
        let parse = |value: Option<String>, label: &str| -> Result<Option<serde_json::Value>> {
            value
                .map(|value| {
                    serde_json::from_str(&value)
                        .with_context(|| format!("该版本保存的 {label} JSON 已损坏"))
                })
                .transpose()
        };
        Ok(AiGenerationDetails {
            version_id: version_id.to_owned(),
            request_body: parse(request_body_json, "请求")?,
            response_body: parse(response_body_json, "响应")?,
        })
    }

    pub fn find_ai_document_version(&self, id: &str) -> Result<AiDocumentVersion> {
        let (mut version, generated_hash) = self
            .connection
            .lock()
            .query_row(
                "SELECT id, document_id, version_number, mode, parent_version_id, status,
                        file_path, provider_name, provider_kind, model_id, template_name,
                        template_revision, transcription_generation, estimated_input_tokens,
                        input_tokens, output_tokens, error_message, created_at, completed_at,
                        generated_hash
                 FROM ai_document_versions WHERE id = ?1",
                [id],
                ai_document_version_from_row,
            )
            .context("找不到该 AI 文档版本")?;
        version.file_state = resolve_ai_file_state(&version, generated_hash.as_deref());
        Ok(version)
    }

    pub fn list_ai_document_versions(&self, document_id: &str) -> Result<Vec<AiDocumentVersion>> {
        let rows = {
            let connection = self.connection.lock();
            let mut statement = connection.prepare(
                "SELECT id, document_id, version_number, mode, parent_version_id, status,
                        file_path, provider_name, provider_kind, model_id, template_name,
                        template_revision, transcription_generation, estimated_input_tokens,
                        input_tokens, output_tokens, error_message, created_at, completed_at,
                        generated_hash
                 FROM ai_document_versions WHERE document_id = ?1
                 ORDER BY version_number DESC",
            )?;
            statement
                .query_map([document_id], ai_document_version_from_row)?
                .filter_map(std::result::Result::ok)
                .collect::<Vec<_>>()
        };
        Ok(rows
            .into_iter()
            .map(|(mut version, generated_hash)| {
                version.file_state = resolve_ai_file_state(&version, generated_hash.as_deref());
                version
            })
            .collect())
    }

    pub fn relink_ai_document_version(&self, id: &str, path: &str) -> Result<AiDocumentVersion> {
        self.connection.lock().execute(
            "UPDATE ai_document_versions SET file_path = ?1 WHERE id = ?2",
            params![path, id],
        )?;
        self.find_ai_document_version(id)
    }

    pub fn ai_document_file_links(
        &self,
        recording_id: &str,
    ) -> Result<Vec<(String, String, String)>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT v.file_path, v.document_id, v.id
             FROM ai_document_versions v
             JOIN ai_documents d ON d.id = v.document_id
             WHERE d.recording_id = ?1
               AND v.status = 'completed'
               AND v.file_path IS NOT NULL",
        )?;
        let rows = statement.query_map([recording_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    pub fn has_active_ai_generation(&self, recording_id: &str) -> Result<bool> {
        let count: i64 = self.connection.lock().query_row(
            "SELECT COUNT(*)
             FROM ai_document_versions v
             JOIN ai_documents d ON d.id = v.document_id
             WHERE d.recording_id = ?1 AND v.status IN ('queued', 'generating')",
            [recording_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn interrupt_running_ai_generations(&self) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE ai_document_versions
             SET status = 'interrupted', error_message = '应用退出时任务尚未完成',
                 completed_at = ?1
             WHERE status IN ('queued', 'generating')",
            [Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn default_ai_workspace_path(&self, recording: &RecordingItem) -> PathBuf {
        let root = self
            .settings()
            .map(|settings| PathBuf::from(settings.ai_documents_directory))
            .unwrap_or_else(|_| self.paths.default_ai_documents.clone());
        let title = sanitize_path_component(&recording.title, "会议");
        let short_id = recording.id.chars().take(8).collect::<String>();
        root.join(format!("{title} [{short_id}]"))
    }

    pub fn begin_transcription(
        &self,
        recording_id: &str,
        provider: &AsrProvider,
        speaker_count: Option<u32>,
    ) -> Result<u32> {
        let capabilities = provider.kind.capabilities();
        if let Some(count) = speaker_count {
            let (Some(min), Some(max)) = (
                capabilities.speaker_count_min,
                capabilities.speaker_count_max,
            ) else {
                bail!("当前转写服务不支持指定说话人数");
            };
            if !(min..=max).contains(&count) {
                bail!("当前转写服务的说话人数必须在 {min} 到 {max} 之间");
            }
        }
        let now = Utc::now().to_rfc3339();
        let protocol = match provider.kind {
            AsrProviderKind::FunAsr => TranscriptionProtocol::NotaBatchV1,
            AsrProviderKind::OpenAiCompatible => TranscriptionProtocol::LegacyChunks,
            AsrProviderKind::DashScope => TranscriptionProtocol::DashScopeFileTransV1,
        };
        let progress_phase = if matches!(
            protocol,
            TranscriptionProtocol::NotaBatchV1 | TranscriptionProtocol::DashScopeFileTransV1
        ) {
            TranscriptionProgressPhase::Uploading
        } else {
            TranscriptionProgressPhase::Preparing
        };
        let idempotency_key = uuid::Uuid::new_v4().to_string();
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
              error_message, created_at, updated_at, completed_at, protocol,
              remote_job_id, idempotency_key, progress_phase, progress_current,
              progress_total, progress_unit, speaker_count, provider_kind, provider_state_json)
             VALUES(?1, ?2, ?3, ?4, ?5, 'queued', 0, 0, '', '[]', NULL, NULL, ?6, ?6, NULL,
                    ?7, NULL, ?8, ?9, 0, 0, ?10, ?11, ?12, '{}')
             ON CONFLICT(recording_id) DO UPDATE SET
               generation = excluded.generation,
               provider_id = excluded.provider_id,
               provider_name = excluded.provider_name,
               provider_kind = excluded.provider_kind,
               model_id = excluded.model_id,
               status = 'queued',
               completed_chunks = 0,
               total_chunks = 0,
               error_message = NULL,
               updated_at = excluded.updated_at,
               completed_at = NULL,
               protocol = excluded.protocol,
               remote_job_id = NULL,
               provider_state_json = '{}',
               idempotency_key = excluded.idempotency_key,
               progress_phase = excluded.progress_phase,
               progress_current = 0,
               progress_total = 0,
               progress_unit = excluded.progress_unit,
               speaker_count = excluded.speaker_count",
            params![
                recording_id,
                generation,
                provider.id,
                provider.name,
                provider.model_id,
                now,
                protocol.as_str(),
                idempotency_key,
                progress_phase.as_str(),
                if matches!(
                    protocol,
                    TranscriptionProtocol::NotaBatchV1
                        | TranscriptionProtocol::DashScopeFileTransV1
                ) {
                    TranscriptionProgressUnit::Bytes.as_str()
                } else {
                    TranscriptionProgressUnit::Chunks.as_str()
                },
                speaker_count,
                provider.kind.as_str(),
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
            "UPDATE transcriptions
             SET status = 'queued',
                 progress_phase = CASE
                   WHEN protocol IN ('nota_batch_v1', 'dashscope_filetrans_v1') THEN 'queued'
                   ELSE 'preparing'
                 END,
                 error_message = NULL, updated_at = ?2
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
        let phase = match status {
            TranscriptionStatus::Queued => TranscriptionProgressPhase::Queued,
            TranscriptionStatus::Preparing => TranscriptionProgressPhase::Preparing,
            _ => TranscriptionProgressPhase::Transcribing,
        };
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET status = ?3, completed_chunks = ?4, total_chunks = ?5,
                 progress_phase = ?6, progress_current = ?4, progress_total = ?5,
                 progress_unit = 'chunks', updated_at = ?7
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                status.as_str(),
                completed_chunks,
                total_chunks,
                phase.as_str(),
                Utc::now().to_rfc3339(),
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
                  progress_phase = NULL,
                  progress_current = CASE
                    WHEN progress_total > 0 THEN progress_total ELSE 1
                  END,
                  progress_total = CASE
                    WHEN progress_total > 0 THEN progress_total ELSE 1
                  END,
                  remote_job_id = CASE
                    WHEN protocol = 'dashscope_filetrans_v1' THEN NULL
                    ELSE remote_job_id
                  END,
                  provider_state_json = '{}',
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
                        error_message, text, protocol, progress_phase,
                        progress_current, progress_total, progress_unit, speaker_count,
                        provider_kind
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

    pub fn transcription_execution(&self, recording_id: &str) -> Result<TranscriptionExecution> {
        self.connection
            .lock()
            .query_row(
                "SELECT protocol, remote_job_id, idempotency_key, speaker_count,
                        provider_kind, provider_state_json
                 FROM transcriptions WHERE recording_id = ?1",
                [recording_id],
                |row| {
                    Ok(TranscriptionExecution {
                        protocol: TranscriptionProtocol::from_str(&row.get::<_, String>(0)?),
                        remote_job_id: row.get(1)?,
                        idempotency_key: row.get(2)?,
                        speaker_count: row.get(3)?,
                        provider_kind: AsrProviderKind::from_str(&row.get::<_, String>(4)?),
                        provider_state_json: row.get(5)?,
                    })
                },
            )
            .context("该录音还没有转写任务")
    }

    pub fn set_remote_transcription_job(
        &self,
        recording_id: &str,
        generation: u32,
        remote_job_id: Option<&str>,
    ) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET remote_job_id = ?3, updated_at = ?4
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                remote_job_id,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn set_transcription_provider_checkpoint(
        &self,
        recording_id: &str,
        generation: u32,
        remote_job_id: Option<&str>,
        provider_state_json: &str,
    ) -> Result<()> {
        serde_json::from_str::<serde_json::Value>(provider_state_json)
            .context("Provider checkpoint 不是有效 JSON")?;
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET remote_job_id = ?3, provider_state_json = ?4, updated_at = ?5
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                remote_job_id,
                provider_state_json,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_batch_transcription_progress(
        &self,
        recording_id: &str,
        generation: u32,
        status: TranscriptionStatus,
        phase: TranscriptionProgressPhase,
        current: u64,
        total: u64,
        unit: TranscriptionProgressUnit,
    ) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE transcriptions
             SET status = ?3, progress_phase = ?4, progress_current = ?5,
                 progress_total = ?6, progress_unit = ?7,
                 completed_chunks = 0, total_chunks = 0, updated_at = ?8
             WHERE recording_id = ?1 AND generation = ?2",
            params![
                recording_id,
                generation,
                status.as_str(),
                phase.as_str(),
                current.min(i64::MAX as u64) as i64,
                total.min(i64::MAX as u64) as i64,
                unit.as_str(),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn transcript(&self, recording_id: &str) -> Result<TranscriptDocument> {
        let connection = self.connection.lock();
        let mut document = connection
            .query_row(
                "SELECT status, provider_name, model_id, language, text, segments_json,
                        completed_chunks, total_chunks, error_message, updated_at,
                        provider_kind, protocol
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
                        speaker_names: std::collections::BTreeMap::new(),
                        speaker_assignments: std::collections::BTreeMap::new(),
                        completed_chunks: row.get::<_, i64>(6)?.max(0) as u32,
                        total_chunks: row.get::<_, i64>(7)?.max(0) as u32,
                        error_message: row.get(8)?,
                        updated_at: row.get(9)?,
                        provider_kind: AsrProviderKind::from_str(&row.get::<_, String>(10)?),
                        protocol: TranscriptionProtocol::from_str(&row.get::<_, String>(11)?),
                        voiceprint_analysis_supported: AsrProviderKind::from_str(
                            &row.get::<_, String>(10)?,
                        )
                        .capabilities()
                        .voiceprint_analysis,
                    })
                },
            )
            .context("该录音还没有转写结果")?;
        let mut statement = connection.prepare(
            "SELECT a.raw_speaker, p.id, p.display_name
             FROM recording_speaker_assignments a
             JOIN participants p ON p.id = a.participant_id
             JOIN transcriptions t
               ON t.recording_id = a.recording_id AND t.generation = a.generation
             WHERE a.recording_id = ?1
             ORDER BY a.raw_speaker",
        )?;
        let rows = statement.query_map([recording_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                RecordingSpeakerAssignment {
                    participant_id: row.get(1)?,
                    display_name: row.get(2)?,
                },
            ))
        })?;
        document.speaker_assignments = rows.filter_map(std::result::Result::ok).collect();
        document.speaker_names = document
            .speaker_assignments
            .iter()
            .map(|(speaker, assignment)| (speaker.clone(), assignment.display_name.clone()))
            .collect();
        Ok(document)
    }

    pub fn voiceprint_embeddings(&self) -> Result<Vec<StoredVoiceprintEmbedding>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT v.participant_id, p.display_name, v.embedding_fingerprint,
                    v.dimension, v.embedding
             FROM voiceprints v
             JOIN participants p ON p.id = v.participant_id
             ORDER BY p.display_name, v.created_at",
        )?;
        let rows = statement.query_map([], |row| {
            let dimension = row.get::<_, i64>(3)?.max(0) as usize;
            let bytes: Vec<u8> = row.get(4)?;
            Ok(StoredVoiceprintEmbedding {
                participant_id: row.get(0)?,
                display_name: row.get(1)?,
                embedding_fingerprint: row.get(2)?,
                dimension,
                embedding: decode_embedding(&bytes, dimension).unwrap_or_default(),
            })
        })?;
        Ok(rows
            .filter_map(std::result::Result::ok)
            .filter(|sample| sample.embedding.len() == sample.dimension)
            .collect())
    }

    pub fn list_participants(&self) -> Result<Vec<ParticipantProfile>> {
        let connection = self.connection.lock();
        let mut participants_statement = connection.prepare(
            "SELECT id, display_name, created_at, updated_at
             FROM participants ORDER BY display_name COLLATE NOCASE",
        )?;
        let participant_rows = participants_statement.query_map([], |row| {
            Ok(ParticipantProfile {
                id: row.get(0)?,
                display_name: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                samples: Vec::new(),
            })
        })?;
        let mut participants = participant_rows
            .filter_map(std::result::Result::ok)
            .collect::<Vec<_>>();

        let mut sample_statement = connection.prepare(
            "SELECT v.id, v.participant_id, v.embedding_fingerprint,
                    v.source_recording_id, r.title, v.source_speaker,
                    v.preview_start_ms, v.preview_end_ms, v.speech_duration_ms,
                    v.created_at, r.path
             FROM voiceprints v
             LEFT JOIN recordings r ON r.id = v.source_recording_id
             ORDER BY v.created_at DESC",
        )?;
        let sample_rows = sample_statement.query_map([], |row| {
            let source_recording_id: Option<String> = row.get(3)?;
            let recording_path: Option<String> = row.get(10)?;
            Ok(VoiceprintSample {
                id: row.get(0)?,
                participant_id: row.get(1)?,
                embedding_fingerprint: row.get(2)?,
                source_recording_id,
                source_recording_title: row.get(4)?,
                source_speaker: row.get(5)?,
                preview_start_ms: row.get::<_, i64>(6)?.max(0) as u64,
                preview_end_ms: row.get::<_, i64>(7)?.max(0) as u64,
                preview_available: recording_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file()),
                speech_duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
                created_at: row.get(9)?,
            })
        })?;
        let samples = sample_rows
            .filter_map(std::result::Result::ok)
            .collect::<Vec<_>>();
        for participant in &mut participants {
            participant.samples = samples
                .iter()
                .filter(|sample| sample.participant_id == participant.id)
                .cloned()
                .collect();
        }
        Ok(participants)
    }

    pub fn rename_participant(&self, id: &str, display_name: &str) -> Result<()> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            bail!("说话人姓名不能为空");
        }
        let changed = self.connection.lock().execute(
            "UPDATE participants SET display_name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, display_name, Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            bail!("找不到该说话人");
        }
        Ok(())
    }

    pub fn delete_participant(&self, id: &str) -> Result<()> {
        let changed = self
            .connection
            .lock()
            .execute("DELETE FROM participants WHERE id = ?1", [id])?;
        if changed == 0 {
            bail!("找不到该说话人");
        }
        Ok(())
    }

    pub fn delete_voiceprint(&self, id: &str) -> Result<()> {
        let changed = self
            .connection
            .lock()
            .execute("DELETE FROM voiceprints WHERE id = ?1", [id])?;
        if changed == 0 {
            bail!("找不到该声纹样本");
        }
        Ok(())
    }

    pub fn update_recording_speaker_assignments(
        &self,
        recording_id: &str,
        assignments: &[SpeakerIdentificationAssignment],
    ) -> Result<()> {
        if assignments.is_empty() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let (generation, status, segments_json) = transaction
            .query_row(
                "SELECT generation, status, segments_json
                 FROM transcriptions WHERE recording_id = ?1",
                [recording_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .context("该录音还没有转写结果")?;
        if TranscriptionStatus::from_str(&status) != TranscriptionStatus::Completed {
            bail!("请先完成这场会议的文字转写");
        }
        let generation = generation.max(1).min(u32::MAX as i64) as u32;
        let segments = serde_json::from_str::<Vec<TranscriptSegment>>(&segments_json)
            .context("当前转写片段无法读取")?;
        let valid_speakers = segments
            .iter()
            .filter_map(|segment| segment.speaker.as_deref())
            .map(str::trim)
            .filter(|speaker| !speaker.is_empty())
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for assignment in assignments {
            let raw_speaker = assignment.raw_speaker.trim();
            if raw_speaker.is_empty() || !valid_speakers.contains(raw_speaker) {
                bail!("当前转写结果中不存在 speaker：{raw_speaker}");
            }
            if !seen.insert(raw_speaker.to_owned()) {
                bail!("同一个 speaker 只能更新一次");
            }
            let participant_id = resolve_assignment_participant(
                &transaction,
                assignment.participant_id.as_deref(),
                assignment.new_display_name.as_deref(),
                &now,
            )?;
            if let Some(participant_id) = participant_id {
                upsert_recording_speaker_assignment(
                    &transaction,
                    RecordingSpeakerAssignmentUpsert {
                        recording_id,
                        generation,
                        raw_speaker,
                        participant_id: &participant_id,
                        match_score: None,
                        assignment_source: "manual",
                        now: &now,
                    },
                )?;
            } else {
                transaction.execute(
                    "DELETE FROM recording_speaker_assignments
                     WHERE recording_id = ?1 AND generation = ?2 AND raw_speaker = ?3",
                    params![recording_id, generation, raw_speaker],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn save_speaker_identification(
        &self,
        recording_id: &str,
        generation: u32,
        enrollments: &[VoiceprintEnrollment],
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        for enrollment in enrollments {
            let raw_speaker = enrollment.raw_speaker.trim();
            let participant_id = resolve_assignment_participant(
                &transaction,
                enrollment.participant_id.as_deref(),
                enrollment.new_display_name.as_deref(),
                &now,
            )?;
            let Some(participant_id) = participant_id else {
                transaction.execute(
                    "DELETE FROM recording_speaker_assignments
                     WHERE recording_id = ?1 AND generation = ?2 AND raw_speaker = ?3",
                    params![recording_id, generation, raw_speaker],
                )?;
                continue;
            };
            upsert_recording_speaker_assignment(
                &transaction,
                RecordingSpeakerAssignmentUpsert {
                    recording_id,
                    generation,
                    raw_speaker,
                    participant_id: &participant_id,
                    match_score: enrollment.match_score,
                    assignment_source: "confirmed",
                    now: &now,
                },
            )?;
            let Some(embedding) = enrollment.embedding.as_deref() else {
                continue;
            };
            if embedding.is_empty() || embedding.iter().any(|value| !value.is_finite()) {
                bail!("声纹向量无效");
            }
            transaction.execute(
                "INSERT INTO voiceprints
                 (id, participant_id, embedding_model, embedding_fingerprint,
                  dimension, embedding, source_recording_id, source_generation,
                  source_speaker, preview_start_ms, preview_end_ms,
                  speech_duration_ms, created_at)
                 VALUES(?1, ?2, 'cam++', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(source_recording_id, source_generation, source_speaker)
                 WHERE source_recording_id IS NOT NULL DO UPDATE SET
                   participant_id = excluded.participant_id,
                   embedding_model = excluded.embedding_model,
                   embedding_fingerprint = excluded.embedding_fingerprint,
                   dimension = excluded.dimension,
                   embedding = excluded.embedding,
                   preview_start_ms = excluded.preview_start_ms,
                   preview_end_ms = excluded.preview_end_ms,
                   speech_duration_ms = excluded.speech_duration_ms,
                   created_at = excluded.created_at",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    participant_id,
                    enrollment.embedding_fingerprint,
                    embedding.len() as i64,
                    encode_embedding(embedding),
                    recording_id,
                    generation,
                    enrollment.raw_speaker,
                    enrollment.preview_start_ms.min(i64::MAX as u64) as i64,
                    enrollment.preview_end_ms.min(i64::MAX as u64) as i64,
                    enrollment.speech_duration_ms.min(i64::MAX as u64) as i64,
                    now,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
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
        let protocol = TranscriptionProtocol::from_str(
            &row.get::<_, Option<String>>(14)?
                .unwrap_or_else(|| "legacy_chunks".into()),
        );
        let progress_phase = row
            .get::<_, Option<String>>(15)?
            .as_deref()
            .and_then(TranscriptionProgressPhase::from_str);
        let progress_current = row.get::<_, Option<i64>>(16)?.unwrap_or(0).max(0) as u64;
        let progress_total = row.get::<_, Option<i64>>(17)?.unwrap_or(0).max(0) as u64;
        let progress_unit = row
            .get::<_, Option<String>>(18)?
            .as_deref()
            .and_then(TranscriptionProgressUnit::from_str);
        let speaker_count = row.get(19)?;
        let provider_kind = AsrProviderKind::from_str(
            &row.get::<_, Option<String>>(20)?
                .unwrap_or_else(|| "open_ai_compatible".into()),
        );
        Some(TranscriptionSummary {
            status: TranscriptionStatus::from_str(&status),
            completed_chunks,
            total_chunks,
            provider_name,
            model_id,
            speaker_count,
            error_message,
            has_text: !text.trim().is_empty(),
            protocol,
            progress_phase,
            progress_current,
            progress_total,
            progress_unit,
            provider_kind,
            voiceprint_analysis_supported: provider_kind.capabilities().voiceprint_analysis,
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
        origin: RecordingOrigin::from_str(
            &row.get::<_, Option<String>>(21)?
                .unwrap_or_else(|| "captured".into()),
        ),
        source_file_name: row.get(22)?,
        source_format: row.get(23)?,
        imported_at: row.get(24)?,
        transcription,
    })
}

fn audio_import_job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AudioImportJob> {
    Ok(AudioImportJob {
        id: row.get(0)?,
        source_path: row.get(1)?,
        source_file_name: row.get(2)?,
        source_format: row.get(3)?,
        source_sha256: row.get(4)?,
        title: row.get(5)?,
        created_at: row.get(6)?,
        imported_at: row.get(7)?,
        final_path: row.get(8)?,
        partial_path: row.get(9)?,
        duration_ms: row
            .get::<_, Option<i64>>(10)?
            .map(|value| value.max(0) as u64),
        size_bytes: row
            .get::<_, Option<i64>>(11)?
            .map(|value| value.max(0) as u64),
        status: row.get(12)?,
    })
}

fn encode_embedding(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(values));
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_embedding(bytes: &[u8], dimension: usize) -> Option<Vec<f32>> {
    if dimension == 0 || bytes.len() != dimension.checked_mul(std::mem::size_of::<f32>())? {
        return None;
    }
    let values = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

fn provider_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AsrProvider> {
    let kind = AsrProviderKind::from_str(&row.get::<_, String>(2)?);
    Ok(AsrProvider {
        id: row.get(0)?,
        name: row.get(1)?,
        kind,
        base_url: row.get(3)?,
        model_id: row.get(4)?,
        has_api_key: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        capabilities: kind.capabilities(),
    })
}

fn llm_provider_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LlmProvider> {
    Ok(LlmProvider {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: LlmProviderKind::from_str(&row.get::<_, String>(2)?),
        base_url: row.get(3)?,
        model_id: row.get(4)?,
        input_token_budget: row.get::<_, i64>(5)?.max(0) as u32,
        max_output_tokens: row.get::<_, i64>(6)?.max(0) as u32,
        has_api_key: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn ai_template_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiTemplate> {
    Ok(AiTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        builtin_key: row.get(3)?,
        task_instructions: row.get(4)?,
        output_requirements: row.get(5)?,
        requires_speaker_labels: row.get::<_, i64>(6)? != 0,
        revision: row.get::<_, i64>(7)?.max(1) as u32,
        archived: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn ensure_ai_template_columns(connection: &Connection) -> Result<()> {
    let columns = {
        let mut statement = connection.prepare("PRAGMA table_info(ai_templates)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        rows.filter_map(std::result::Result::ok)
            .collect::<std::collections::HashSet<_>>()
    };
    if !columns.contains("requires_speaker_labels") {
        connection.execute(
            "ALTER TABLE ai_templates
             ADD COLUMN requires_speaker_labels INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn ensure_ai_generation_detail_columns(connection: &Connection) -> Result<()> {
    let columns = {
        let mut statement = connection.prepare("PRAGMA table_info(ai_document_versions)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        rows.filter_map(std::result::Result::ok)
            .collect::<std::collections::HashSet<_>>()
    };
    if !columns.contains("request_body_json") {
        connection.execute(
            "ALTER TABLE ai_document_versions ADD COLUMN request_body_json TEXT",
            [],
        )?;
    }
    if !columns.contains("response_body_json") {
        connection.execute(
            "ALTER TABLE ai_document_versions ADD COLUMN response_body_json TEXT",
            [],
        )?;
    }
    Ok(())
}

fn ai_meeting_profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiMeetingProfile> {
    Ok(AiMeetingProfile {
        recording_id: row.get(0)?,
        workspace_path: row.get(1)?,
        meeting_context: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn ai_document_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiDocument> {
    Ok(AiDocument {
        id: row.get(0)?,
        recording_id: row.get(1)?,
        template_id: row.get(2)?,
        title: row.get(3)?,
        requirements: row.get(4)?,
        template_name: row.get(5)?,
        template_builtin_key: row.get(6)?,
        latest_version: None,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn ai_document_version_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(AiDocumentVersion, Option<String>)> {
    Ok((
        AiDocumentVersion {
            id: row.get(0)?,
            document_id: row.get(1)?,
            version_number: row.get::<_, i64>(2)?.max(1) as u32,
            mode: AiGenerationMode::from_str(&row.get::<_, String>(3)?),
            parent_version_id: row.get(4)?,
            status: AiGenerationStatus::from_str(&row.get::<_, String>(5)?),
            file_path: row.get(6)?,
            file_state: AiFileState::Pending,
            provider_name: row.get(7)?,
            provider_kind: LlmProviderKind::from_str(&row.get::<_, String>(8)?),
            model_id: row.get(9)?,
            template_name: row.get(10)?,
            template_revision: row.get::<_, i64>(11)?.max(1) as u32,
            transcription_generation: row.get::<_, i64>(12)?.max(0) as u32,
            estimated_input_tokens: row.get::<_, i64>(13)?.max(0) as u32,
            input_tokens: row
                .get::<_, Option<i64>>(14)?
                .map(|value| value.max(0) as u32),
            output_tokens: row
                .get::<_, Option<i64>>(15)?
                .map(|value| value.max(0) as u32),
            error_message: row.get(16)?,
            created_at: row.get(17)?,
            completed_at: row.get(18)?,
        },
        row.get(19)?,
    ))
}

fn resolve_ai_file_state(version: &AiDocumentVersion, generated_hash: Option<&str>) -> AiFileState {
    if version.status != AiGenerationStatus::Completed {
        return AiFileState::Pending;
    }
    let Some(path) = version.file_path.as_deref() else {
        return AiFileState::Missing;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return AiFileState::Missing;
    };
    let Some(expected) = generated_hash else {
        return AiFileState::Modified;
    };
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected {
        AiFileState::Ready
    } else {
        AiFileState::Modified
    }
}

fn validate_llm_provider(request: &SaveLlmProviderRequest) -> Result<()> {
    if request.name.trim().is_empty() {
        bail!("服务名称不能为空");
    }
    if request.model_id.trim().is_empty() {
        bail!("模型 ID 不能为空");
    }
    if request.input_token_budget < 1_024 || request.input_token_budget > 2_000_000 {
        bail!("输入 token 预算必须在 1024 到 2000000 之间");
    }
    if request.max_output_tokens < 256 || request.max_output_tokens > 131_072 {
        bail!("最大输出 token 必须在 256 到 131072 之间");
    }
    if request.base_url.trim().is_empty() {
        bail!("AI 模型服务必须填写 API 根地址");
    }
    Ok(())
}

fn validate_ai_template(request: &SaveAiTemplateRequest) -> Result<()> {
    if request.name.trim().is_empty() {
        bail!("模板名称不能为空");
    }
    if request.name.chars().count() > 80 {
        bail!("模板名称不能超过 80 个字符");
    }
    if request.task_instructions.trim().is_empty() {
        bail!("模板任务指令不能为空");
    }
    if request.task_instructions.chars().count() > 20_000
        || request.output_requirements.chars().count() > 20_000
    {
        bail!("模板指令过长");
    }
    Ok(())
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
        speaker_count: row.get(12)?,
        error_message: row.get(5)?,
        has_text: !text.trim().is_empty(),
        protocol: TranscriptionProtocol::from_str(&row.get::<_, String>(7)?),
        progress_phase: row
            .get::<_, Option<String>>(8)?
            .as_deref()
            .and_then(TranscriptionProgressPhase::from_str),
        progress_current: row.get::<_, i64>(9)?.max(0) as u64,
        progress_total: row.get::<_, i64>(10)?.max(0) as u64,
        progress_unit: row
            .get::<_, Option<String>>(11)?
            .as_deref()
            .and_then(TranscriptionProgressUnit::from_str),
        provider_kind: AsrProviderKind::from_str(&row.get::<_, String>(13)?),
        voiceprint_analysis_supported: AsrProviderKind::from_str(&row.get::<_, String>(13)?)
            .capabilities()
            .voiceprint_analysis,
    })
}

fn ensure_recording_import_columns(connection: &Connection) -> Result<()> {
    let columns = {
        let mut statement = connection.prepare("PRAGMA table_info(recordings)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        rows.filter_map(std::result::Result::ok)
            .collect::<std::collections::HashSet<_>>()
    };
    let additions = [
        (
            "origin",
            "ALTER TABLE recordings ADD COLUMN origin TEXT NOT NULL DEFAULT 'captured'",
        ),
        (
            "source_file_name",
            "ALTER TABLE recordings ADD COLUMN source_file_name TEXT",
        ),
        (
            "source_format",
            "ALTER TABLE recordings ADD COLUMN source_format TEXT",
        ),
        (
            "source_sha256",
            "ALTER TABLE recordings ADD COLUMN source_sha256 TEXT",
        ),
        (
            "imported_at",
            "ALTER TABLE recordings ADD COLUMN imported_at TEXT",
        ),
    ];
    for (name, sql) in additions {
        if !columns.contains(name) {
            connection.execute(sql, [])?;
        }
    }
    Ok(())
}

fn ensure_audio_import_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS recordings_import_hash_unique
           ON recordings(source_sha256)
           WHERE origin = 'imported' AND source_sha256 IS NOT NULL;
         CREATE TABLE IF NOT EXISTS audio_import_jobs (
           id TEXT PRIMARY KEY,
           source_path TEXT NOT NULL,
           source_file_name TEXT NOT NULL,
           source_format TEXT,
           source_sha256 TEXT,
           title TEXT NOT NULL,
           created_at TEXT NOT NULL,
           imported_at TEXT NOT NULL,
           final_path TEXT NOT NULL,
           partial_path TEXT NOT NULL,
           duration_ms INTEGER,
           size_bytes INTEGER,
           status TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );",
    )?;
    Ok(())
}

fn ensure_transcription_job_columns(connection: &Connection) -> Result<()> {
    let columns = {
        let mut statement = connection.prepare("PRAGMA table_info(transcriptions)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        rows.filter_map(std::result::Result::ok)
            .collect::<std::collections::HashSet<_>>()
    };
    let additions = [
        (
            "protocol",
            "ALTER TABLE transcriptions ADD COLUMN protocol TEXT NOT NULL DEFAULT 'legacy_chunks'",
        ),
        (
            "remote_job_id",
            "ALTER TABLE transcriptions ADD COLUMN remote_job_id TEXT",
        ),
        (
            "idempotency_key",
            "ALTER TABLE transcriptions ADD COLUMN idempotency_key TEXT NOT NULL DEFAULT ''",
        ),
        (
            "progress_phase",
            "ALTER TABLE transcriptions ADD COLUMN progress_phase TEXT",
        ),
        (
            "progress_current",
            "ALTER TABLE transcriptions ADD COLUMN progress_current INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "progress_total",
            "ALTER TABLE transcriptions ADD COLUMN progress_total INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "progress_unit",
            "ALTER TABLE transcriptions ADD COLUMN progress_unit TEXT",
        ),
        (
            "speaker_count",
            "ALTER TABLE transcriptions ADD COLUMN speaker_count INTEGER CHECK(speaker_count BETWEEN 1 AND 100)",
        ),
        (
            "provider_kind",
            "ALTER TABLE transcriptions ADD COLUMN provider_kind TEXT NOT NULL DEFAULT 'open_ai_compatible'",
        ),
        (
            "provider_state_json",
            "ALTER TABLE transcriptions ADD COLUMN provider_state_json TEXT NOT NULL DEFAULT '{}'",
        ),
    ];
    for (name, sql) in additions {
        if !columns.contains(name) {
            connection.execute(sql, [])?;
        }
    }
    connection.execute(
        "UPDATE transcriptions
         SET provider_kind = CASE protocol
           WHEN 'nota_batch_v1' THEN 'fun_asr'
           WHEN 'dashscope_filetrans_v1' THEN 'dash_scope'
           ELSE 'open_ai_compatible'
         END
         WHERE provider_kind = '' OR provider_kind = 'open_ai_compatible'",
        [],
    )?;
    Ok(())
}

fn ensure_transcription_speaker_count_constraint(connection: &Connection) -> Result<()> {
    let schema = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'transcriptions'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_default();
    if !schema.contains("BETWEEN 1 AND 64") {
        return Ok(());
    }
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         ALTER TABLE transcriptions RENAME TO transcriptions_legacy;
         CREATE TABLE transcriptions (
           recording_id TEXT PRIMARY KEY REFERENCES recordings(id) ON DELETE CASCADE,
           generation INTEGER NOT NULL DEFAULT 1,
           provider_id TEXT,
           provider_name TEXT NOT NULL,
           provider_kind TEXT NOT NULL DEFAULT 'open_ai_compatible',
           model_id TEXT NOT NULL,
           speaker_count INTEGER CHECK(speaker_count BETWEEN 1 AND 100),
           status TEXT NOT NULL,
           completed_chunks INTEGER NOT NULL DEFAULT 0,
           total_chunks INTEGER NOT NULL DEFAULT 0,
           text TEXT NOT NULL DEFAULT '',
           segments_json TEXT NOT NULL DEFAULT '[]',
           language TEXT,
           error_message TEXT,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           completed_at TEXT,
           protocol TEXT NOT NULL DEFAULT 'legacy_chunks',
           remote_job_id TEXT,
           provider_state_json TEXT NOT NULL DEFAULT '{}',
           idempotency_key TEXT NOT NULL DEFAULT '',
           progress_phase TEXT,
           progress_current INTEGER NOT NULL DEFAULT 0,
           progress_total INTEGER NOT NULL DEFAULT 0,
           progress_unit TEXT
         );
         INSERT INTO transcriptions (
           recording_id, generation, provider_id, provider_name, provider_kind, model_id,
           speaker_count, status, completed_chunks, total_chunks, text, segments_json,
           language, error_message, created_at, updated_at, completed_at, protocol,
           remote_job_id, provider_state_json, idempotency_key, progress_phase,
           progress_current, progress_total, progress_unit
         )
         SELECT recording_id, generation, provider_id, provider_name, provider_kind, model_id,
                speaker_count, status, completed_chunks, total_chunks, text, segments_json,
                language, error_message, created_at, updated_at, completed_at, protocol,
                remote_job_id, provider_state_json, idempotency_key, progress_phase,
                progress_current, progress_total, progress_unit
         FROM transcriptions_legacy;
         DROP TABLE transcriptions_legacy;
         COMMIT;",
    )?;
    Ok(())
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

pub(crate) fn sanitize_path_component(value: &str, fallback: &str) -> String {
    let sanitized = value
        .trim()
        .trim_end_matches(['.', ' '])
        .chars()
        .map(|character| {
            if r#"<>:"/\|?*"#.contains(character) || character.is_control() {
                '_'
            } else {
                character
            }
        })
        .take(80)
        .collect::<String>();
    let sanitized = sanitized.trim().trim_end_matches(['.', ' ']);
    if sanitized.is_empty() {
        fallback.to_owned()
    } else {
        sanitized.to_owned()
    }
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
            default_ai_documents: nota.join("AI Documents"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database: nota.join("nota.db"),
        })
        .unwrap();
        (root, storage)
    }

    #[test]
    fn audio_import_commit_records_origin_metadata_and_enables_deduplication() {
        let (root, storage) = test_storage();
        let recordings = root.join("Nota").join("Recordings");
        let final_path = recordings.join("phone-meeting.ogg");
        let partial_path = recordings.join(".nota-import-test.partial.ogg");
        std::fs::write(&final_path, b"normalized audio").unwrap();
        let job = AudioImportJob {
            id: "imported-meeting".into(),
            source_path: "C:\\Phone\\meeting.m4a".into(),
            source_file_name: "meeting.m4a".into(),
            source_format: None,
            source_sha256: None,
            title: "手机会议".into(),
            created_at: "2026-08-10T09:00:00Z".into(),
            imported_at: "2026-08-11T09:00:00Z".into(),
            final_path: final_path.to_string_lossy().into_owned(),
            partial_path: partial_path.to_string_lossy().into_owned(),
            duration_ms: None,
            size_bytes: None,
            status: "writing".into(),
        };
        storage.begin_audio_import(&job).unwrap();
        storage
            .prepare_audio_import("imported-meeting", "M4A", "abc123", 65_000, 16)
            .unwrap();
        storage
            .mark_audio_import_file_committed("imported-meeting")
            .unwrap();
        let recording = storage.complete_audio_import("imported-meeting").unwrap();

        assert_eq!(recording.origin, RecordingOrigin::Imported);
        assert_eq!(recording.source_file_name.as_deref(), Some("meeting.m4a"));
        assert_eq!(recording.source_format.as_deref(), Some("M4A"));
        assert_eq!(
            recording.imported_at.as_deref(),
            Some("2026-08-11T09:00:00Z")
        );
        assert_eq!(recording.duration_ms, 65_000);
        assert_eq!(
            storage
                .imported_recording_by_hash("abc123")
                .unwrap()
                .unwrap()
                .id,
            "imported-meeting"
        );
        assert!(storage.audio_import_jobs().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
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

    fn llm_provider_request(
        id: Option<String>,
        api_key: AsrApiKeyUpdate,
    ) -> SaveLlmProviderRequest {
        SaveLlmProviderRequest {
            id,
            name: "OpenAI".into(),
            kind: LlmProviderKind::OpenAi,
            base_url: "https://api.openai.com/v1".into(),
            model_id: "test-model".into(),
            input_token_budget: 32_768,
            max_output_tokens: 4_096,
            api_key,
        }
    }

    #[test]
    fn ai_templates_seed_four_stable_builtins() {
        let (root, storage) = test_storage();
        let templates = storage.list_ai_templates().unwrap();
        let mut keys = templates
            .iter()
            .filter_map(|template| template.builtin_key.clone())
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "action_items",
                "meeting_summary",
                "speaker_standup",
                "speaker_summary"
            ]
        );
        let builtin = templates
            .iter()
            .find(|template| template.builtin_key.as_deref() == Some("meeting_summary"))
            .unwrap();
        assert!(storage.archive_ai_template(&builtin.id).is_err());
        let speaker = templates
            .iter()
            .find(|template| template.builtin_key.as_deref() == Some("speaker_summary"))
            .unwrap();
        assert!(speaker.requires_speaker_labels);
        let cloned = storage
            .clone_ai_template(&speaker.id, "Custom speaker summary")
            .unwrap();
        assert!(cloned.requires_speaker_labels);

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_ai_template_tables_gain_the_speaker_requirement_column() {
        let root = std::env::temp_dir().join(format!(
            "nota-ai-template-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let nota = root.join("Nota");
        let recordings = nota.join("Recordings");
        let recovery = nota.join("Recovery");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::create_dir_all(&recovery).unwrap();
        let database = nota.join("nota.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE ai_templates (
                   id TEXT PRIMARY KEY,
                   name TEXT NOT NULL,
                   description TEXT NOT NULL DEFAULT '',
                   builtin_key TEXT,
                   task_instructions TEXT NOT NULL,
                   output_requirements TEXT NOT NULL,
                   revision INTEGER NOT NULL DEFAULT 1,
                   archived INTEGER NOT NULL DEFAULT 0,
                   created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL
                 );",
            )
            .unwrap();
        drop(connection);

        let storage = Storage::open(AppPaths {
            recovery,
            logs: nota.join("Logs"),
            default_ai_documents: nota.join("AI Documents"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database,
        })
        .unwrap();
        let speaker = storage
            .list_ai_templates()
            .unwrap()
            .into_iter()
            .find(|template| template.builtin_key.as_deref() == Some("speaker_summary"))
            .unwrap();
        assert!(speaker.requires_speaker_labels);

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_ai_version_tables_gain_generation_detail_columns() {
        let root = std::env::temp_dir().join(format!(
            "nota-ai-generation-detail-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let nota = root.join("Nota");
        let recordings = nota.join("Recordings");
        let recovery = nota.join("Recovery");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::create_dir_all(&recovery).unwrap();
        let database = nota.join("nota.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE ai_document_versions (id TEXT PRIMARY KEY);")
            .unwrap();
        drop(connection);

        let storage = Storage::open(AppPaths {
            recovery,
            logs: nota.join("Logs"),
            default_ai_documents: nota.join("AI Documents"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database,
        })
        .unwrap();
        let columns = {
            let connection = storage.connection.lock();
            let mut statement = connection
                .prepare("PRAGMA table_info(ai_document_versions)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(std::result::Result::ok)
                .collect::<std::collections::HashSet<_>>()
        };
        assert!(columns.contains("request_body_json"));
        assert!(columns.contains("response_body_json"));

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn llm_provider_keys_stay_in_rust_and_support_keep_and_clear() {
        let (root, storage) = test_storage();
        let saved = storage
            .save_llm_provider(llm_provider_request(
                None,
                AsrApiKeyUpdate::Replace {
                    value: "secret-key".into(),
                },
            ))
            .unwrap();
        assert!(saved.has_api_key);
        assert!(storage.list_llm_providers().unwrap()[0].has_api_key);
        assert_eq!(
            storage.find_llm_provider(&saved.id).unwrap().api_key,
            "secret-key"
        );

        let kept = storage
            .save_llm_provider(llm_provider_request(
                Some(saved.id.clone()),
                AsrApiKeyUpdate::Keep,
            ))
            .unwrap();
        assert!(kept.has_api_key);
        assert_eq!(
            storage.find_llm_provider(&saved.id).unwrap().api_key,
            "secret-key"
        );

        let cleared = storage
            .save_llm_provider(llm_provider_request(
                Some(saved.id.clone()),
                AsrApiKeyUpdate::Clear,
            ))
            .unwrap();
        assert!(!cleared.has_api_key);
        assert!(
            storage
                .find_llm_provider(&saved.id)
                .unwrap()
                .api_key
                .is_empty()
        );

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn custom_ai_templates_are_revisioned_and_archived_and_unused_providers_can_be_deleted() {
        let (root, storage) = test_storage();
        let first = storage
            .save_ai_template(SaveAiTemplateRequest {
                id: None,
                name: "Decision log".into(),
                description: "Track decisions".into(),
                task_instructions: "Extract decisions".into(),
                output_requirements: "Use a table".into(),
                requires_speaker_labels: false,
            })
            .unwrap();
        assert_eq!(first.revision, 1);

        let revised = storage
            .save_ai_template(SaveAiTemplateRequest {
                id: Some(first.id.clone()),
                name: "Decision log".into(),
                description: "Track decisions and owners".into(),
                task_instructions: "Extract decisions with their speakers".into(),
                output_requirements: "Use a table with an owner column".into(),
                requires_speaker_labels: true,
            })
            .unwrap();
        assert_eq!(revised.revision, 2);
        assert!(revised.requires_speaker_labels);

        storage.archive_ai_template(&first.id).unwrap();
        let archived = storage
            .list_ai_templates()
            .unwrap()
            .into_iter()
            .find(|template| template.id == first.id)
            .unwrap();
        assert!(archived.archived);

        let provider = storage
            .save_llm_provider(llm_provider_request(
                None,
                AsrApiKeyUpdate::Replace {
                    value: "secret-key".into(),
                },
            ))
            .unwrap();
        storage.delete_llm_provider(&provider.id).unwrap();
        assert!(storage.list_llm_providers().unwrap().is_empty());

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ai_workspace_uses_the_current_configured_root_for_new_versions() {
        let (root, storage) = test_storage();
        let recording_path = storage.paths().default_recordings.join("workspace.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        storage
            .insert_recording(&RecordingItem {
                id: "workspace-meeting".into(),
                title: "Workspace meeting".into(),
                path: recording_path.to_string_lossy().into_owned(),
                created_at: "2026-08-10T00:00:00Z".into(),
                duration_ms: 10_000,
                size_bytes: 5,
                recovered: false,
                origin: RecordingOrigin::Captured,
                source_file_name: None,
                source_format: None,
                imported_at: None,
                transcription: None,
            })
            .unwrap();

        let original = storage.ai_workspace("workspace-meeting").unwrap();
        storage
            .save_ai_meeting_profile(
                "workspace-meeting",
                &original.profile.workspace_path,
                "Preserved meeting context",
            )
            .unwrap();

        let custom_root = root.join("Custom AI Documents");
        let mut settings = storage.settings().unwrap();
        settings.ai_documents_directory = custom_root.to_string_lossy().into_owned();
        storage.save_settings(&settings).unwrap();

        let updated = storage.ai_workspace("workspace-meeting").unwrap();
        assert!(PathBuf::from(&updated.profile.workspace_path).starts_with(&custom_root));
        assert_eq!(updated.profile.meeting_context, "Preserved meeting context");

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ai_documents_are_one_per_template_with_append_only_file_versions() {
        let (root, storage) = test_storage();
        let recording_path = storage.paths().default_recordings.join("meeting.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        storage
            .insert_recording(&RecordingItem {
                id: "ai-meeting".into(),
                title: "Weekly meeting".into(),
                path: recording_path.to_string_lossy().into_owned(),
                created_at: "2026-08-09T00:00:00Z".into(),
                duration_ms: 10_000,
                size_bytes: 5,
                recovered: false,
                origin: RecordingOrigin::Captured,
                source_file_name: None,
                source_format: None,
                imported_at: None,
                transcription: None,
            })
            .unwrap();
        let provider = storage
            .save_llm_provider(llm_provider_request(None, AsrApiKeyUpdate::Clear))
            .unwrap();
        let template = storage
            .list_ai_templates()
            .unwrap()
            .into_iter()
            .find(|template| template.builtin_key.as_deref() == Some("meeting_summary"))
            .unwrap();
        let document = storage
            .create_ai_document("ai-meeting", &template.id, "Summary", "For the team")
            .unwrap();
        assert!(
            storage
                .create_ai_document("ai-meeting", &template.id, "Duplicate", "")
                .is_err()
        );

        let first_path = root.join("summary-v001.md");
        let first_markdown = format!(
            "---\nnota:\n  document_id: \"{}\"\n  version_id: \"v1\"\n---\n\n# Summary\n",
            document.id
        );
        std::fs::write(&first_path, &first_markdown).unwrap();
        let first = storage
            .insert_ai_document_version(NewAiVersion {
                id: "v1",
                document_id: &document.id,
                version_number: storage.next_ai_version_number(&document.id).unwrap(),
                mode: AiGenerationMode::Create,
                parent_version_id: None,
                file_path: first_path.to_string_lossy().as_ref(),
                provider_id: &provider.id,
                provider: &provider,
                template: &template,
                transcription_generation: 1,
                speaker_names_json: "{}",
                meeting_context: "Project context",
                document_requirements: "For the team",
                run_request: "",
                estimated_input_tokens: 120,
                request_body_json: r#"{"model":"test-model","input":"meeting"}"#,
            })
            .unwrap();
        assert_eq!(first.version_number, 1);
        let first_hash = format!("{:x}", Sha256::digest(first_markdown.as_bytes()));
        let completed = storage
            .complete_ai_document_version(
                "v1",
                &first_hash,
                Some(100),
                Some(20),
                r#"{"usage":{"input_tokens":100,"output_tokens":20}}"#,
            )
            .unwrap();
        assert_eq!(completed.file_state, AiFileState::Ready);
        let details = storage.find_ai_generation_details("v1").unwrap();
        assert_eq!(details.request_body.unwrap()["model"], "test-model");
        assert_eq!(details.response_body.unwrap()["usage"]["output_tokens"], 20);

        std::fs::write(&first_path, format!("{first_markdown}\nExternal edit\n")).unwrap();
        assert_eq!(
            storage.find_ai_document_version("v1").unwrap().file_state,
            AiFileState::Modified
        );

        let second_path = root.join("summary-v002.md");
        let second = storage
            .insert_ai_document_version(NewAiVersion {
                id: "v2",
                document_id: &document.id,
                version_number: storage.next_ai_version_number(&document.id).unwrap(),
                mode: AiGenerationMode::Regenerate,
                parent_version_id: None,
                file_path: second_path.to_string_lossy().as_ref(),
                provider_id: &provider.id,
                provider: &provider,
                template: &template,
                transcription_generation: 1,
                speaker_names_json: "{}",
                meeting_context: "Project context",
                document_requirements: "For the team",
                run_request: "Shorter",
                estimated_input_tokens: 125,
                request_body_json: r#"{"model":"test-model","input":"shorter"}"#,
            })
            .unwrap();
        assert_eq!(second.version_number, 2);
        storage
            .set_ai_version_status("v2", AiGenerationStatus::Failed, Some("synthetic failure"))
            .unwrap();
        let failed_details = storage.find_ai_generation_details("v2").unwrap();
        assert!(failed_details.request_body.is_some());
        assert!(failed_details.response_body.is_none());
        assert_eq!(
            storage.ai_document_file_links("ai-meeting").unwrap(),
            vec![(
                first_path.to_string_lossy().into_owned(),
                document.id.clone(),
                "v1".to_owned(),
            )]
        );
        assert_eq!(storage.next_ai_version_number(&document.id).unwrap(), 3);
        let versions = storage.list_ai_document_versions(&document.id).unwrap();
        assert_eq!(
            versions
                .iter()
                .map(|version| version.id.as_str())
                .collect::<Vec<_>>(),
            vec!["v2", "v1"]
        );

        std::fs::remove_file(&first_path).unwrap();
        assert_eq!(
            storage.find_ai_document_version("v1").unwrap().file_state,
            AiFileState::Missing
        );
        let columns = {
            let connection = storage.connection.lock();
            let mut statement = connection
                .prepare("PRAGMA table_info(ai_document_versions)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(std::result::Result::ok)
                .collect::<Vec<_>>()
        };
        assert!(
            !columns
                .iter()
                .any(|column| column == "body" || column == "markdown")
        );

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn voiceprints_resolve_names_without_rewriting_raw_segments() {
        let (root, storage) = test_storage();
        let recording_path = storage.paths().default_recordings.join("meeting.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        storage
            .insert_recording(&RecordingItem {
                id: "meeting".into(),
                title: "产品周会".into(),
                path: recording_path.to_string_lossy().into_owned(),
                created_at: "2026-08-02T00:00:00Z".into(),
                duration_ms: 10_000,
                size_bytes: 5,
                recovered: false,
                origin: RecordingOrigin::Captured,
                source_file_name: None,
                source_format: None,
                imported_at: None,
                transcription: None,
            })
            .unwrap();
        let provider = storage
            .save_asr_provider(provider_request(None, AsrApiKeyUpdate::Clear))
            .unwrap();
        let generation = storage
            .begin_transcription("meeting", &provider, None)
            .unwrap();
        storage
            .complete_transcription(
                "meeting",
                generation,
                "大家好",
                &[
                    TranscriptSegment {
                        start_ms: 0,
                        end_ms: 6_000,
                        text: "大家好".into(),
                        speaker: Some("speaker_0".into()),
                    },
                    TranscriptSegment {
                        start_ms: 6_000,
                        end_ms: 10_000,
                        text: "你好".into(),
                        speaker: Some("speaker_1".into()),
                    },
                ],
                Some("zh"),
            )
            .unwrap();
        storage
            .save_speaker_identification(
                "meeting",
                generation,
                &[VoiceprintEnrollment {
                    raw_speaker: "speaker_0".into(),
                    participant_id: None,
                    new_display_name: Some("小明".into()),
                    match_score: None,
                    embedding_fingerprint: "cam++:test:v1".into(),
                    embedding: Some(vec![0.6, 0.8]),
                    preview_start_ms: 0,
                    preview_end_ms: 6_000,
                    speech_duration_ms: 6_000,
                }],
            )
            .unwrap();

        let transcript = storage.transcript("meeting").unwrap();
        assert_eq!(transcript.segments[0].speaker.as_deref(), Some("speaker_0"));
        assert_eq!(transcript.speaker_names.get("speaker_0").unwrap(), "小明");
        assert_eq!(
            transcript
                .speaker_assignments
                .get("speaker_0")
                .unwrap()
                .display_name,
            "小明"
        );
        let profiles = storage.list_participants().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].samples.len(), 1);
        assert!(profiles[0].samples[0].preview_available);
        let embeddings = storage.voiceprint_embeddings().unwrap();
        assert_eq!(embeddings[0].embedding, vec![0.6, 0.8]);

        storage
            .update_recording_speaker_assignments(
                "meeting",
                &[SpeakerIdentificationAssignment {
                    raw_speaker: "speaker_1".into(),
                    participant_id: None,
                    new_display_name: Some("小红".into()),
                }],
            )
            .unwrap();
        storage
            .save_speaker_identification(
                "meeting",
                generation,
                &[VoiceprintEnrollment {
                    raw_speaker: "speaker_0".into(),
                    participant_id: Some(profiles[0].id.clone()),
                    new_display_name: None,
                    match_score: None,
                    embedding_fingerprint: String::new(),
                    embedding: None,
                    preview_start_ms: 0,
                    preview_end_ms: 6_000,
                    speech_duration_ms: 6_000,
                }],
            )
            .unwrap();
        let transcript = storage.transcript("meeting").unwrap();
        assert_eq!(transcript.speaker_names.get("speaker_0").unwrap(), "小明");
        assert_eq!(transcript.speaker_names.get("speaker_1").unwrap(), "小红");

        storage
            .update_recording_speaker_assignments(
                "meeting",
                &[SpeakerIdentificationAssignment {
                    raw_speaker: "speaker_1".into(),
                    participant_id: None,
                    new_display_name: None,
                }],
            )
            .unwrap();
        let transcript = storage.transcript("meeting").unwrap();
        assert_eq!(transcript.speaker_names.get("speaker_0").unwrap(), "小明");
        assert!(!transcript.speaker_names.contains_key("speaker_1"));
        assert!(
            storage
                .update_recording_speaker_assignments(
                    "meeting",
                    &[SpeakerIdentificationAssignment {
                        raw_speaker: "speaker_99".into(),
                        participant_id: None,
                        new_display_name: Some("不存在".into()),
                    }],
                )
                .is_err()
        );

        storage
            .rename_participant(&profiles[0].id, "小明同学")
            .unwrap();
        assert_eq!(
            storage
                .transcript("meeting")
                .unwrap()
                .speaker_names
                .get("speaker_0")
                .unwrap(),
            "小明同学"
        );
        storage
            .delete_voiceprint(&profiles[0].samples[0].id)
            .unwrap();
        assert_eq!(storage.list_participants().unwrap()[0].samples.len(), 0);
        assert_eq!(
            storage.transcript("meeting").unwrap().speaker_names.len(),
            1
        );
        storage
            .save_speaker_identification(
                "meeting",
                generation,
                &[VoiceprintEnrollment {
                    raw_speaker: "speaker_0".into(),
                    participant_id: Some(profiles[0].id.clone()),
                    new_display_name: None,
                    match_score: None,
                    embedding_fingerprint: "cam++:test:v1".into(),
                    embedding: None,
                    preview_start_ms: 0,
                    preview_end_ms: 6_000,
                    speech_duration_ms: 6_000,
                }],
            )
            .unwrap();
        assert_eq!(storage.list_participants().unwrap()[0].samples.len(), 0);
        assert_eq!(
            storage
                .transcript("meeting")
                .unwrap()
                .speaker_names
                .get("speaker_0")
                .unwrap(),
            "小明同学"
        );
        storage.delete_participant(&profiles[0].id).unwrap();
        assert!(
            storage
                .transcript("meeting")
                .unwrap()
                .speaker_names
                .is_empty()
        );

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
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
            default_ai_documents: root.join("Nota").join("AI Documents"),
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
            default_ai_documents: PathBuf::from(r"C:\Users\user\Documents\Nota\AI Documents"),
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
            default_ai_documents: nota.join("AI Documents"),
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
            origin: RecordingOrigin::Captured,
            source_file_name: None,
            source_format: None,
            imported_at: None,
            transcription: None,
        };
        storage.insert_recording(&recording).unwrap();
        let provider = storage
            .save_asr_provider(provider_request(None, AsrApiKeyUpdate::Keep))
            .unwrap();
        let generation = storage
            .begin_transcription(&recording.id, &provider, None)
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

    #[test]
    fn funasr_jobs_persist_batch_identity_and_generic_progress() {
        let (root, storage) = test_storage();
        let recording_path = storage.paths.default_recordings.join("batch.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        let recording = RecordingItem {
            id: "batch-recording".into(),
            title: "batch".into(),
            path: recording_path.to_string_lossy().into_owned(),
            created_at: "2026-07-28T00:00:00Z".into(),
            duration_ms: 10_000,
            size_bytes: 5,
            recovered: false,
            origin: RecordingOrigin::Captured,
            source_file_name: None,
            source_format: None,
            imported_at: None,
            transcription: None,
        };
        storage.insert_recording(&recording).unwrap();
        let provider = storage
            .save_asr_provider(provider_request(None, AsrApiKeyUpdate::Keep))
            .unwrap();
        let invalid_count = storage
            .begin_transcription(&recording.id, &provider, Some(65))
            .unwrap_err();
        assert!(format!("{invalid_count:#}").contains("1 到 64"));
        let mut openai_provider = provider.clone();
        openai_provider.kind = AsrProviderKind::OpenAiCompatible;
        let unsupported = storage
            .begin_transcription(&recording.id, &openai_provider, Some(3))
            .unwrap_err();
        assert!(format!("{unsupported:#}").contains("不支持指定说话人数"));
        let generation = storage
            .begin_transcription(&recording.id, &provider, Some(3))
            .unwrap();

        let execution = storage.transcription_execution(&recording.id).unwrap();
        assert_eq!(execution.protocol, TranscriptionProtocol::NotaBatchV1);
        assert!(!execution.idempotency_key.is_empty());
        assert_eq!(execution.remote_job_id, None);
        assert_eq!(execution.speaker_count, Some(3));
        assert_eq!(
            storage
                .transcription_summary(&recording.id)
                .unwrap()
                .speaker_count,
            Some(3)
        );

        storage
            .set_remote_transcription_job(&recording.id, generation, Some("remote-job"))
            .unwrap();
        storage
            .set_batch_transcription_progress(
                &recording.id,
                generation,
                TranscriptionStatus::Transcribing,
                TranscriptionProgressPhase::Diarizing,
                3,
                3,
                TranscriptionProgressUnit::Windows,
            )
            .unwrap();
        let execution = storage.transcription_execution(&recording.id).unwrap();
        let summary = storage.transcription_summary(&recording.id).unwrap();
        assert_eq!(execution.remote_job_id.as_deref(), Some("remote-job"));
        assert_eq!(summary.protocol, TranscriptionProtocol::NotaBatchV1);
        assert_eq!(
            summary.progress_phase,
            Some(TranscriptionProgressPhase::Diarizing)
        );
        assert_eq!(summary.progress_current, 3);
        assert_eq!(summary.progress_total, 3);
        assert_eq!(
            summary.progress_unit,
            Some(TranscriptionProgressUnit::Windows)
        );
        assert_eq!(summary.completed_chunks, 0);
        assert_eq!(summary.total_chunks, 0);

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dashscope_jobs_snapshot_capabilities_and_clear_temporary_state_on_commit() {
        let (root, storage) = test_storage();
        let recording_path = storage.paths.default_recordings.join("dashscope.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        let recording = RecordingItem {
            id: "dashscope-recording".into(),
            title: "dashscope".into(),
            path: recording_path.to_string_lossy().into_owned(),
            created_at: "2026-08-22T00:00:00Z".into(),
            duration_ms: 10_000,
            size_bytes: 5,
            recovered: false,
            origin: RecordingOrigin::Captured,
            source_file_name: None,
            source_format: None,
            imported_at: None,
            transcription: None,
        };
        storage.insert_recording(&recording).unwrap();
        let mut provider = storage
            .save_asr_provider(provider_request(None, AsrApiKeyUpdate::Keep))
            .unwrap();
        provider.kind = AsrProviderKind::DashScope;
        provider.capabilities = provider.kind.capabilities();
        provider.base_url = "https://dashscope.aliyuncs.com/api/v1".into();
        provider.model_id = "qwen-audio-3.0-asr-flash-filetrans".into();

        assert!(
            storage
                .begin_transcription(&recording.id, &provider, Some(1))
                .is_err()
        );
        assert!(
            storage
                .begin_transcription(&recording.id, &provider, Some(101))
                .is_err()
        );
        let generation = storage
            .begin_transcription(&recording.id, &provider, Some(100))
            .unwrap();
        storage
            .set_transcription_provider_checkpoint(
                &recording.id,
                generation,
                Some("task-id"),
                r#"{"version":1,"stage":"running"}"#,
            )
            .unwrap();

        let execution = storage.transcription_execution(&recording.id).unwrap();
        assert_eq!(
            execution.protocol,
            TranscriptionProtocol::DashScopeFileTransV1
        );
        assert_eq!(execution.provider_kind, AsrProviderKind::DashScope);
        assert_eq!(execution.remote_job_id.as_deref(), Some("task-id"));
        let summary = storage.transcription_summary(&recording.id).unwrap();
        assert_eq!(summary.provider_kind, AsrProviderKind::DashScope);
        assert!(!summary.voiceprint_analysis_supported);

        storage
            .complete_transcription(&recording.id, generation, "完成", &[], Some("zh"))
            .unwrap();
        let execution = storage.transcription_execution(&recording.id).unwrap();
        assert_eq!(execution.remote_job_id, None);
        assert_eq!(execution.provider_state_json, "{}");
        let transcript = storage.transcript(&recording.id).unwrap();
        assert_eq!(transcript.provider_kind, AsrProviderKind::DashScope);
        assert!(!transcript.voiceprint_analysis_supported);

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn old_transcription_rows_migrate_to_legacy_chunk_protocol() {
        let root = std::env::temp_dir().join(format!(
            "nota-storage-batch-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let nota = root.join("Nota");
        let recordings = nota.join("Recordings");
        let recovery = nota.join("Recovery");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::create_dir_all(&recovery).unwrap();
        let recording_path = recordings.join("legacy.ogg");
        std::fs::write(&recording_path, b"audio").unwrap();
        let database = nota.join("nota.db");
        {
            let connection = Connection::open(&database).unwrap();
            connection
                .execute_batch(
                    "
                    CREATE TABLE recordings (
                      id TEXT PRIMARY KEY,
                      title TEXT NOT NULL,
                      path TEXT NOT NULL UNIQUE,
                      created_at TEXT NOT NULL,
                      duration_ms INTEGER NOT NULL,
                      size_bytes INTEGER NOT NULL,
                      recovered INTEGER NOT NULL DEFAULT 0
                    );
                    CREATE TABLE transcriptions (
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
                    ",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO recordings
                     (id, title, path, created_at, duration_ms, size_bytes, recovered)
                     VALUES('legacy', 'legacy', ?1, '2026-01-01T00:00:00Z', 1000, 5, 0)",
                    [recording_path.to_string_lossy().as_ref()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO transcriptions
                     (recording_id, provider_name, model_id, status, completed_chunks,
                      total_chunks, text, created_at, updated_at)
                     VALUES('legacy', 'old service', 'old-model', 'interrupted', 1, 2,
                            '', '2026-01-01T00:00:00Z', '2026-01-01T00:01:00Z')",
                    [],
                )
                .unwrap();
        }
        let storage = Storage::open(AppPaths {
            recovery,
            logs: nota.join("Logs"),
            default_ai_documents: nota.join("AI Documents"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database,
        })
        .unwrap();
        let columns = {
            let connection = storage.connection.lock();
            let mut statement = connection
                .prepare("PRAGMA table_info(transcriptions)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(std::result::Result::ok)
                .collect::<std::collections::HashSet<_>>()
        };
        assert!(columns.contains("protocol"));
        assert!(columns.contains("remote_job_id"));
        assert!(columns.contains("idempotency_key"));
        assert!(columns.contains("progress_phase"));
        assert!(columns.contains("progress_current"));
        assert!(columns.contains("progress_total"));
        assert!(columns.contains("progress_unit"));
        assert!(columns.contains("speaker_count"));
        assert!(columns.contains("provider_kind"));
        assert!(columns.contains("provider_state_json"));
        let summary = storage.transcription_summary("legacy").unwrap();
        assert_eq!(summary.protocol, TranscriptionProtocol::LegacyChunks);
        assert_eq!(summary.provider_kind, AsrProviderKind::OpenAiCompatible);
        assert_eq!(summary.speaker_count, None);
        assert_eq!(summary.completed_chunks, 1);
        assert_eq!(summary.total_chunks, 2);
        let legacy_recording = storage.find_recording("legacy").unwrap();
        assert_eq!(legacy_recording.origin, RecordingOrigin::Captured);
        assert_eq!(legacy_recording.source_file_name, None);
        assert_eq!(legacy_recording.source_format, None);
        assert_eq!(legacy_recording.imported_at, None);

        drop(storage);
        std::fs::remove_dir_all(root).unwrap();
    }
}
