use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub recovery: PathBuf,
    pub logs: PathBuf,
    pub default_recordings: PathBuf,
    pub default_ai_documents: PathBuf,
    pub legacy_default_recordings: PathBuf,
    pub database: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let local_data = dirs::data_local_dir().context("无法定位 LocalAppData")?;
        let legacy_data = local_data.join("Meeting Note");
        let nota_data = local_data.join("Nota");
        let data = migrate_directory_or_keep_legacy(&legacy_data, &nota_data);

        let documents = dirs::document_dir().unwrap_or_else(|| data.clone());
        let legacy_default_recordings = documents.join("Meeting Note").join("Recordings");
        let nota_default_recordings = documents.join("Nota").join("Recordings");
        let default_ai_documents = documents.join("Nota").join("AI Documents");
        let default_recordings =
            migrate_directory_or_keep_legacy(&legacy_default_recordings, &nota_default_recordings);

        let database = if data == legacy_data {
            data.join("meeting-note.db")
        } else {
            migrate_sqlite_family(&data)?;
            data.join("nota.db")
        };
        let recovery = data.join("Recovery");
        let logs = data.join("Logs");
        if data != legacy_data {
            migrate_log_files(&logs)?;
        }
        for directory in [&data, &recovery, &logs, &default_recordings] {
            std::fs::create_dir_all(directory)
                .with_context(|| format!("无法创建目录 {}", directory.display()))?;
        }
        Ok(Self {
            recovery,
            logs,
            default_recordings,
            default_ai_documents,
            legacy_default_recordings,
            database,
        })
    }
}

fn migrate_directory_or_keep_legacy(legacy: &Path, nota: &Path) -> PathBuf {
    if nota.exists() {
        return nota.to_path_buf();
    }
    if legacy.exists() {
        if let Some(parent) = nota.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return legacy.to_path_buf();
        }
        return match std::fs::rename(legacy, nota) {
            Ok(()) => nota.to_path_buf(),
            Err(_) => legacy.to_path_buf(),
        };
    }
    nota.to_path_buf()
}

fn migrate_sqlite_family(directory: &Path) -> Result<()> {
    let legacy_database = directory.join("meeting-note.db");
    let nota_database = directory.join("nota.db");
    if nota_database.exists() || !legacy_database.exists() {
        return Ok(());
    }

    let mut moved = Vec::new();
    for suffix in ["-wal", "-shm", ""] {
        let source = directory.join(format!("meeting-note.db{suffix}"));
        if !source.exists() {
            continue;
        }
        let destination = directory.join(format!("nota.db{suffix}"));
        if destination.exists() {
            rollback_moves(&moved);
            anyhow::bail!("Nota 数据库迁移目标已存在：{}", destination.display());
        }
        if let Err(error) = std::fs::rename(&source, &destination) {
            rollback_moves(&moved);
            return Err(error).with_context(|| {
                format!(
                    "无法迁移数据库 {} 到 {}",
                    source.display(),
                    destination.display()
                )
            });
        }
        moved.push((source, destination));
    }
    Ok(())
}

fn rollback_moves(moved: &[(PathBuf, PathBuf)]) {
    for (source, destination) in moved.iter().rev() {
        let _ = std::fs::rename(destination, source);
    }
}

fn migrate_log_files(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for suffix in ["", ".1", ".2"] {
        let source = directory.join(format!("meeting-note{suffix}.log"));
        let destination = directory.join(format!("nota{suffix}.log"));
        if source.exists() && !destination.exists() {
            std::fs::rename(&source, &destination).with_context(|| {
                format!(
                    "无法迁移日志 {} 到 {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_a_legacy_directory_without_losing_contents() {
        let root = std::env::temp_dir().join(format!("nota-path-test-{}", uuid::Uuid::new_v4()));
        let legacy = root
            .join("Documents")
            .join("Meeting Note")
            .join("Recordings");
        let nota = root.join("Documents").join("Nota").join("Recordings");
        std::fs::create_dir_all(legacy.join("Recovery")).unwrap();
        std::fs::write(
            legacy.join("Recovery").join("session.partial.ogg"),
            b"audio",
        )
        .unwrap();

        let selected = migrate_directory_or_keep_legacy(&legacy, &nota);

        assert_eq!(selected, nota);
        assert_eq!(
            std::fs::read(nota.join("Recovery").join("session.partial.ogg")).unwrap(),
            b"audio"
        );
        std::fs::remove_file(nota.join("Recovery").join("session.partial.ogg")).unwrap();
        std::fs::remove_dir(nota.join("Recovery")).unwrap();
        std::fs::remove_dir(nota).unwrap();
        std::fs::remove_dir(root.join("Documents").join("Nota")).unwrap();
        std::fs::remove_dir(root.join("Documents").join("Meeting Note")).unwrap();
        std::fs::remove_dir(root.join("Documents")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn migrates_sqlite_wal_and_shm_as_one_family() {
        let root = std::env::temp_dir().join(format!("nota-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for suffix in ["", "-wal", "-shm"] {
            std::fs::write(
                root.join(format!("meeting-note.db{suffix}")),
                suffix.as_bytes(),
            )
            .unwrap();
        }

        migrate_sqlite_family(&root).unwrap();

        for suffix in ["", "-wal", "-shm"] {
            assert!(!root.join(format!("meeting-note.db{suffix}")).exists());
            assert!(root.join(format!("nota.db{suffix}")).exists());
            std::fs::remove_file(root.join(format!("nota.db{suffix}"))).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
}
