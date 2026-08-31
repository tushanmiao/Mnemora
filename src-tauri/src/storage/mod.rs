use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const STORAGE_LOCATION_VERSION: u32 = 1;
const STORAGE_MARKER_VERSION: u32 = 1;
const LOCATION_FILE_NAME: &str = "storage-location.json";
const MIGRATION_FILE_NAME: &str = "storage-migration.json";
const MIGRATION_RESULT_FILE_NAME: &str = "storage-migration-result.json";
const STORAGE_MARKER_FILE_NAME: &str = ".mnemora-storage.json";
const BLOCKED_RUNTIME_FILE_NAME: &str = "storage-unavailable.block";

/// 受管 SQLite 数据库在数据目录下的相对路径。
///
/// 集中一处而不是在校验函数里各写一遍：迁移的检查点、完整性校验、伴生文件清理
/// 必须覆盖同一批库，任何一处漏掉就是一个静默的数据风险。
const MANAGED_DATABASES: &[&str] = &["library/library.sqlite3", "english/learning.sqlite3"];

/// WAL 模式下 SQLite 产生的伴生文件后缀。
///
/// 这两个文件必须与主库同进同退：
/// - 复制时只拷主库会丢掉还在 WAL 里的已提交事务；
/// - 恢复时把**陈旧**的 `-wal` 留在原地，SQLite 打开时会重放它，静默把刚恢复的
///   数据覆盖回旧状态 —— 这种失败没有任何报错，最难查。
const SQLITE_SIDECAR_SUFFIXES: &[&str] = &["-wal", "-shm"];

const MANAGED_DIRECTORIES: &[(&str, &str)] = &[
    ("conversations", "conversations"),
    ("library", "library"),
    ("memory", "memory"),
    ("prompts", "prompts"),
    ("skills", "skills"),
    ("usage", "usage"),
    ("sync", "sync"),
    ("english", "english"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageLocationFile {
    version: u32,
    custom_data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageMarker {
    version: u32,
    application: String,
    created_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum MigrationPhase {
    Prepared,
    Committing,
    InstallComplete,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingMigration {
    version: u32,
    id: String,
    source: PathBuf,
    destination: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    requested_at: u64,
    phase: MigrationPhase,
    entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageMigrationResult {
    pub succeeded: bool,
    pub source_path: String,
    pub destination_path: String,
    pub completed_at: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCategoryUsage {
    pub id: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub current_path: String,
    pub default_path: String,
    pub is_custom: bool,
    pub available: bool,
    pub availability_error: Option<String>,
    pub total_bytes: u64,
    pub categories: Vec<StorageCategoryUsage>,
    pub previous_path: Option<String>,
    pub last_migration: Option<StorageMigrationResult>,
}

#[derive(Clone)]
pub struct StorageManager {
    config_dir: PathBuf,
    default_data_dir: PathBuf,
    configured_data_dir: PathBuf,
    runtime_data_dir: PathBuf,
    available: bool,
    availability_error: Option<String>,
}

impl StorageManager {
    pub fn bootstrap(config_dir: PathBuf, default_data_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&config_dir).map_err(|error| {
            format!("Failed to create the application configuration directory: {error}")
        })?;

        if let Some(mut pending) =
            read_optional_json::<PendingMigration>(&config_dir.join(MIGRATION_FILE_NAME))?
        {
            let result =
                match execute_pending_migration(&config_dir, &default_data_dir, &mut pending) {
                    Ok(()) => StorageMigrationResult {
                        succeeded: true,
                        source_path: pending.source.to_string_lossy().into_owned(),
                        destination_path: pending.destination.to_string_lossy().into_owned(),
                        completed_at: now_millis(),
                        error: None,
                    },
                    Err(error) => {
                        let rollback_error = if pending.phase == MigrationPhase::Committing {
                            rollback_install(&pending).err()
                        } else {
                            None
                        };
                        let error = rollback_error
                            .map(|rollback| format!("{error}; rollback failed: {rollback}"))
                            .unwrap_or(error);
                        StorageMigrationResult {
                            succeeded: false,
                            source_path: pending.source.to_string_lossy().into_owned(),
                            destination_path: pending.destination.to_string_lossy().into_owned(),
                            completed_at: now_millis(),
                            error: Some(error),
                        }
                    }
                };
            write_json_atomic(&config_dir.join(MIGRATION_RESULT_FILE_NAME), &result)?;
            let preserve_for_recovery = !result.succeeded
                && (pending.phase == MigrationPhase::InstallComplete
                    || pending.phase == MigrationPhase::Committing);
            if !preserve_for_recovery {
                remove_path_if_exists(&pending.staging)?;
                remove_path_if_exists(&pending.backup)?;
                remove_path_if_exists(&config_dir.join(MIGRATION_FILE_NAME))?;
            }
        }

        let location = load_location(&config_dir)?;
        let configured_data_dir = location
            .custom_data_dir
            .unwrap_or_else(|| default_data_dir.clone());
        let is_custom = !paths_equal(&configured_data_dir, &default_data_dir);
        let availability_error = if is_custom {
            validate_active_custom_storage(&configured_data_dir).err()
        } else {
            None
        };
        let available = availability_error.is_none();
        let runtime_data_dir = if available {
            configured_data_dir.clone()
        } else {
            blocked_runtime_path(&config_dir)?
        };

        Ok(Self {
            config_dir,
            default_data_dir,
            configured_data_dir,
            runtime_data_dir,
            available,
            availability_error,
        })
    }

    pub fn runtime_data_dir(&self) -> &Path {
        &self.runtime_data_dir
    }

    pub fn current_data_dir(&self) -> &Path {
        &self.configured_data_dir
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn status(&self) -> Result<StorageStatus, String> {
        let categories = MANAGED_DIRECTORIES
            .iter()
            .map(|(id, directory)| {
                let bytes = if self.available {
                    directory_size(&self.configured_data_dir.join(directory))?
                } else {
                    0
                };
                Ok(StorageCategoryUsage {
                    id: (*id).to_string(),
                    bytes,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let total_bytes = categories.iter().map(|category| category.bytes).sum();
        let last_migration = read_optional_json::<StorageMigrationResult>(
            &self.config_dir.join(MIGRATION_RESULT_FILE_NAME),
        )?;
        let previous_path = last_migration.as_ref().and_then(|migration| {
            (migration.succeeded
                && migration.source_path != self.configured_data_dir.to_string_lossy())
            .then(|| migration.source_path.clone())
        });

        Ok(StorageStatus {
            current_path: self.configured_data_dir.to_string_lossy().into_owned(),
            default_path: self.default_data_dir.to_string_lossy().into_owned(),
            is_custom: !paths_equal(&self.configured_data_dir, &self.default_data_dir),
            available: self.available,
            availability_error: self.availability_error.clone(),
            total_bytes,
            categories,
            previous_path,
            last_migration,
        })
    }

    pub fn prepare_migration(&self, destination: PathBuf) -> Result<(), String> {
        if !self.available {
            return Err(self
                .availability_error
                .clone()
                .unwrap_or_else(|| "The current data directory is unavailable.".to_string()));
        }
        let destination = normalize_destination(destination)?;
        validate_migration_paths(
            &self.configured_data_dir,
            &destination,
            &self.default_data_dir,
        )?;
        let id = Uuid::new_v4().to_string();
        let parent = destination
            .parent()
            .ok_or_else(|| "The selected data directory has no parent directory.".to_string())?
            .to_path_buf();
        let leaf = destination
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("mnemora-data")
            .to_string();
        let pending = PendingMigration {
            version: STORAGE_LOCATION_VERSION,
            id: id.clone(),
            source: self.configured_data_dir.clone(),
            destination,
            staging: parent.join(format!(".{leaf}.mnemora-stage-{id}")),
            backup: parent.join(format!(".{leaf}.mnemora-backup-{id}")),
            requested_at: now_millis(),
            phase: MigrationPhase::Prepared,
            entries: managed_entries(&self.configured_data_dir),
        };
        write_json_atomic(&self.config_dir.join(MIGRATION_FILE_NAME), &pending)
    }
}

fn execute_pending_migration(
    config_dir: &Path,
    default_data_dir: &Path,
    pending: &mut PendingMigration,
) -> Result<(), String> {
    if pending.version != STORAGE_LOCATION_VERSION {
        return Err("The pending storage migration uses an unsupported version.".to_string());
    }
    if pending.phase == MigrationPhase::Committed {
        return Ok(());
    }
    if pending.phase == MigrationPhase::InstallComplete {
        save_location(config_dir, default_data_dir, &pending.destination)?;
        pending.phase = MigrationPhase::Committed;
        return Ok(());
    }
    if pending.phase == MigrationPhase::Committing {
        rollback_install(pending)?;
        pending.phase = MigrationPhase::Prepared;
        write_json_atomic(&config_dir.join(MIGRATION_FILE_NAME), pending)?;
    }

    validate_source_for_migration(&pending.source)?;
    prepare_destination_parent(&pending.destination)?;
    remove_path_if_exists(&pending.staging)?;
    remove_path_if_exists(&pending.backup)?;
    fs::create_dir_all(&pending.staging).map_err(|error| {
        format!("Failed to create storage migration staging directory: {error}")
    })?;

    // 复制前先把源库的 WAL 落盘并清空。
    //
    // `copy_tree` 是逐文件 `fs::copy`，主库和 `-wal` 是两个不同时刻的快照；WAL 开着
    // 的时候，中间只要落进一次写，拷出来的就是一份撕裂的三件套 —— 恢复出来的库要么
    // 少事务，要么 `quick_check` 直接失败。先 TRUNCATE 检查点，主库就是自洽的，
    // `-wal` 是空的，逐文件复制才重新变得安全。
    checkpoint_managed_databases(&pending.source)?;
    for entry in &pending.entries {
        copy_tree(&pending.source.join(entry), &pending.staging.join(entry))?;
    }
    write_json_atomic(
        &pending.staging.join(STORAGE_MARKER_FILE_NAME),
        &StorageMarker {
            version: STORAGE_MARKER_VERSION,
            application: "com.mnemora.app".to_string(),
            created_at: now_millis(),
        },
    )?;
    verify_migration_copy(pending)?;

    pending.phase = MigrationPhase::Committing;
    write_json_atomic(&config_dir.join(MIGRATION_FILE_NAME), pending)?;
    commit_install(pending)?;
    pending.phase = MigrationPhase::InstallComplete;
    write_json_atomic(&config_dir.join(MIGRATION_FILE_NAME), pending)?;
    save_location(config_dir, default_data_dir, &pending.destination)?;
    pending.phase = MigrationPhase::Committed;
    // The location file is the commit point. Do not persist another journal phase after it:
    // a crash here leaves `InstallComplete`, which is intentionally safe and idempotent to replay.
    Ok(())
}

fn commit_install(pending: &PendingMigration) -> Result<(), String> {
    fs::create_dir_all(&pending.destination)
        .map_err(|error| format!("Failed to create the destination data directory: {error}"))?;
    fs::create_dir_all(&pending.backup)
        .map_err(|error| format!("Failed to create the storage rollback directory: {error}"))?;

    for entry in install_entries(pending) {
        let staged = pending.staging.join(&entry);
        let destination = pending.destination.join(&entry);
        let backup = pending.backup.join(&entry);
        if destination.exists() {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("Failed to prepare storage rollback directory: {error}")
                })?;
            }
            fs::rename(&destination, &backup).map_err(|error| {
                format!("Failed to back up existing storage entry {entry}: {error}")
            })?;
        }
        fs::rename(&staged, &destination)
            .map_err(|error| format!("Failed to install storage entry {entry}: {error}"))?;
    }
    Ok(())
}

fn rollback_install(pending: &PendingMigration) -> Result<(), String> {
    if pending.phase != MigrationPhase::Committing {
        return Ok(());
    }
    for entry in install_entries(pending).into_iter().rev() {
        let staged = pending.staging.join(&entry);
        let destination = pending.destination.join(&entry);
        let backup = pending.backup.join(&entry);
        if backup.exists() {
            remove_path_if_exists(&destination)?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("Failed to restore storage parent directory: {error}")
                })?;
            }
            fs::rename(&backup, &destination)
                .map_err(|error| format!("Failed to restore storage entry {entry}: {error}"))?;
        } else if !staged.exists() && destination.exists() {
            remove_path_if_exists(&destination)?;
        }
    }
    Ok(())
}

fn install_entries(pending: &PendingMigration) -> Vec<String> {
    let mut entries = pending.entries.clone();
    entries.push(STORAGE_MARKER_FILE_NAME.to_string());
    entries
}

fn verify_migration_copy(pending: &PendingMigration) -> Result<(), String> {
    let source = build_inventory(&pending.source, &pending.entries)?;
    let destination = build_inventory(&pending.staging, &pending.entries)?;
    if source != destination {
        return Err(
            "Storage migration verification failed because copied files do not match the source."
                .to_string(),
        );
    }
    // 指纹比对必须在打开数据库**之前**完成：打开一个带 WAL 的库会触发恢复，
    // 那会改写暂存副本，比对就永远对不上了。
    for relative in MANAGED_DATABASES {
        verify_sqlite_database(&pending.staging.join(relative))?;
    }
    Ok(())
}

/// 对源目录里的每个受管库做 TRUNCATE 检查点。
///
/// 单个库失败不阻断整次迁移：库可能不存在（首次启动）、可能被别的进程占用。
/// 这一步是**优化复制的一致性**，不是正确性的唯一依赖 —— 伴生文件本身也会被
/// `copy_tree` 一起拷走，所以退化情况下仍有兜底。
fn checkpoint_managed_databases(root: &Path) -> Result<(), String> {
    for relative in MANAGED_DATABASES {
        let path = root.join(relative);
        if !path.is_file() {
            continue;
        }
        let Ok(connection) = Connection::open(&path) else {
            continue;
        };
        // 忽略返回值：非 WAL 模式下这条 PRAGMA 是空操作，报错也无需中断迁移。
        let _ = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }
    Ok(())
}

/// 删除某个库残留的 `-wal` / `-shm`。
///
/// 用在「已经确认主库自洽」之后：留着陈旧伴生文件，SQLite 下次打开会重放它们，
/// 把刚装好的数据静默回滚。
fn remove_sqlite_sidecars(database_path: &Path) -> Result<(), String> {
    let Some(name) = database_path.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    for suffix in SQLITE_SIDECAR_SUFFIXES {
        remove_path_if_exists(&database_path.with_file_name(format!("{name}{suffix}")))?;
    }
    Ok(())
}

fn verify_sqlite_database(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let connection = Connection::open(path).map_err(|error| {
        format!(
            "Failed to open migrated database {}: {error}",
            path.display()
        )
    })?;
    let result: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| {
            format!(
                "Failed to verify migrated database {}: {error}",
                path.display()
            )
        })?;
    if !result.eq_ignore_ascii_case("ok") {
        return Err(format!(
            "Migrated database {} failed integrity verification: {result}",
            path.display()
        ));
    }
    // 这三步的顺序是有依赖的，不能重排：
    //
    // 1. 上面的 `Connection::open` 已经让 SQLite 重放了副本里的 `-wal`（如果源库
    //    检查点没做成、WAL 里还有真实事务，恢复发生在这一步）；
    // 2. 这里的 TRUNCATE 检查点把重放后的内容全部折进主库文件；
    // 3. 于是伴生文件此刻确定是空的，删掉不丢任何数据。
    //
    // 反过来先删再检查点，就会真的丢事务。删除它们的理由是：陈旧的 `-wal` 被带进
    // 目标目录后，下次打开会被重放，静默把刚装好的数据回滚。
    let _ = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    drop(connection);
    remove_sqlite_sidecars(path)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FileFingerprint {
    path: String,
    bytes: u64,
    sha256: String,
}

fn build_inventory(root: &Path, entries: &[String]) -> Result<Vec<FileFingerprint>, String> {
    let mut inventory = Vec::new();
    for entry in entries {
        collect_inventory(root, &root.join(entry), &mut inventory)?;
    }
    inventory.sort();
    Ok(inventory)
}

fn collect_inventory(
    root: &Path,
    path: &Path,
    inventory: &mut Vec<FileFingerprint>,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Failed to inspect storage entry {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Storage migration does not follow symbolic links: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        let mut children = fs::read_dir(path)
            .map_err(|error| {
                format!(
                    "Failed to read storage directory {}: {error}",
                    path.display()
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to enumerate storage directory: {error}"))?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            collect_inventory(root, &child.path(), inventory)?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!("Unsupported storage entry: {}", path.display()));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Failed to calculate storage-relative path.".to_string())?;
    inventory.push(FileFingerprint {
        path: relative.to_string_lossy().replace('\\', "/"),
        bytes: metadata.len(),
        sha256: hash_file(path)?,
    });
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        format!(
            "Failed to inspect source storage entry {}: {error}",
            source.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Storage migration does not copy symbolic links: {}",
            source.display()
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(|error| {
            format!(
                "Failed to create migrated directory {}: {error}",
                destination.display()
            )
        })?;
        let mut children = fs::read_dir(source)
            .map_err(|error| {
                format!(
                    "Failed to read source directory {}: {error}",
                    source.display()
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to enumerate source storage directory: {error}"))?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            copy_tree(&child.path(), &destination.join(child.file_name()))?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!(
            "Unsupported source storage entry: {}",
            source.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create migrated file directory: {error}"))?;
    }
    fs::copy(source, destination).map_err(|error| {
        format!(
            "Failed to copy storage file {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("Failed to open storage file {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read storage file {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn directory_size(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect storage path {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path).map_err(|error| {
        format!(
            "Failed to read storage directory {}: {error}",
            path.display()
        )
    })? {
        total = total.saturating_add(directory_size(
            &entry
                .map_err(|error| format!("Failed to enumerate storage directory: {error}"))?
                .path(),
        )?);
    }
    Ok(total)
}

fn managed_entries(root: &Path) -> Vec<String> {
    MANAGED_DIRECTORIES
        .iter()
        .map(|(_, directory)| *directory)
        .filter(|directory| root.join(directory).exists())
        .map(str::to_string)
        .collect()
}

fn validate_source_for_migration(source: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("The current data directory is unavailable: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The current data path is not a safe local directory.".to_string());
    }
    Ok(())
}

fn validate_active_custom_storage(path: &Path) -> Result<(), String> {
    validate_source_for_migration(path)?;
    let marker: StorageMarker = read_json(&path.join(STORAGE_MARKER_FILE_NAME))?;
    if marker.version != STORAGE_MARKER_VERSION || marker.application != "com.mnemora.app" {
        return Err(
            "The configured custom directory is not a recognized Mnemora data directory."
                .to_string(),
        );
    }
    Ok(())
}

fn normalize_destination(path: PathBuf) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err("Select an absolute data directory path.".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("The data directory must not contain parent path segments.".to_string());
    }
    Ok(path)
}

fn validate_migration_paths(
    source: &Path,
    destination: &Path,
    default_data_dir: &Path,
) -> Result<(), String> {
    if paths_equal(source, destination) {
        return Err("The selected directory is already the current data directory.".to_string());
    }
    if !paths_equal(destination, default_data_dir)
        && destination
            .ancestors()
            .any(|ancestor| ancestor.join(LOCATION_FILE_NAME).is_file())
    {
        return Err(
            "Do not place Mnemora data inside another Mnemora configuration directory.".to_string(),
        );
    }
    let source_canonical = fs::canonicalize(source)
        .map_err(|error| format!("Failed to resolve current data directory: {error}"))?;
    let destination_canonical = canonicalize_destination(destination)?;
    if path_starts_with(&destination_canonical, &source_canonical)
        || path_starts_with(&source_canonical, &destination_canonical)
    {
        return Err(
            "The source and destination data directories must not contain each other.".to_string(),
        );
    }

    if destination.exists() {
        let metadata = fs::symlink_metadata(destination)
            .map_err(|error| format!("Failed to inspect the selected directory: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("The selected data path is not a safe directory.".to_string());
        }
        if !paths_equal(destination, default_data_dir)
            && fs::read_dir(destination)
                .map_err(|error| format!("Failed to read the selected directory: {error}"))?
                .next()
                .is_some()
        {
            return Err("Choose an empty directory for the new Mnemora data location.".to_string());
        }
    }
    prepare_destination_parent(destination)
}

fn canonicalize_destination(destination: &Path) -> Result<PathBuf, String> {
    if destination.exists() {
        return fs::canonicalize(destination)
            .map_err(|error| format!("Failed to resolve selected data directory: {error}"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "The selected data directory has no parent directory.".to_string())?;
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("Failed to resolve selected directory parent: {error}"))?;
    let name = destination
        .file_name()
        .ok_or_else(|| "The selected data directory has no name.".to_string())?;
    Ok(parent.join(name))
}

fn prepare_destination_parent(destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "The selected data directory has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create the destination parent directory: {error}"))?;
    let probe = parent.join(format!(".mnemora-write-probe-{}", Uuid::new_v4()));
    fs::write(&probe, b"mnemora")
        .map_err(|error| format!("The selected destination is not writable: {error}"))?;
    fs::remove_file(&probe)
        .map_err(|error| format!("Failed to remove destination write probe: {error}"))
}

fn blocked_runtime_path(config_dir: &Path) -> Result<PathBuf, String> {
    let path = config_dir.join(BLOCKED_RUNTIME_FILE_NAME);
    if path.is_dir() {
        fs::remove_dir_all(&path)
            .map_err(|error| format!("Failed to reset unavailable-storage blocker: {error}"))?;
    }
    if !path.exists() {
        fs::write(&path, b"Mnemora storage is unavailable.")
            .map_err(|error| format!("Failed to create unavailable-storage blocker: {error}"))?;
    }
    Ok(path)
}

fn load_location(config_dir: &Path) -> Result<StorageLocationFile, String> {
    let path = config_dir.join(LOCATION_FILE_NAME);
    let Some(location) = read_optional_json::<StorageLocationFile>(&path)? else {
        return Ok(StorageLocationFile {
            version: STORAGE_LOCATION_VERSION,
            custom_data_dir: None,
        });
    };
    if location.version != STORAGE_LOCATION_VERSION {
        return Err("The storage location file uses an unsupported version.".to_string());
    }
    Ok(location)
}

fn save_location(
    config_dir: &Path,
    default_data_dir: &Path,
    destination: &Path,
) -> Result<(), String> {
    write_json_atomic(
        &config_dir.join(LOCATION_FILE_NAME),
        &StorageLocationFile {
            version: STORAGE_LOCATION_VERSION,
            custom_data_dir: (!paths_equal(default_data_dir, destination))
                .then(|| destination.to_path_buf()),
        },
    )
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, String> {
    if path.exists() {
        return match read_json(path) {
            Ok(value) => Ok(Some(value)),
            Err(primary_error) => {
                let backup = path.with_extension("json.bak");
                if !backup.exists() {
                    return Err(primary_error);
                }
                read_json(&backup).map(Some).map_err(|backup_error| {
                    format!(
                        "{primary_error}; storage metadata backup is also unreadable: {backup_error}"
                    )
                })
            }
        };
    }
    let backup = path.with_extension("json.bak");
    if backup.exists() {
        return read_json(&backup).map(Some);
    }
    Ok(None)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Failed to parse {}: {error}", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Storage metadata path has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create storage metadata directory: {error}"))?;
    let temporary =
        path.with_extension(format!("json.tmp-{}-{}", std::process::id(), now_millis()));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize storage metadata: {error}"))?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("Failed to create storage metadata temporary file: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Failed to write storage metadata: {error}"))?;
    drop(file);
    let backup = path.with_extension("json.bak");
    if path.exists() {
        remove_path_if_exists(&backup)?;
        fs::rename(path, &backup)
            .map_err(|error| format!("Failed to back up storage metadata: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("Failed to install storage metadata: {error}"));
    }
    remove_path_if_exists(&backup)
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect path {}: {error}", path.display()))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
            .map_err(|error| format!("Failed to remove directory {}: {error}", path.display()))
    } else {
        fs::remove_file(path)
            .map_err(|error| format!("Failed to remove file {}: {error}", path.display()))
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        left.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(right.to_string_lossy().trim_end_matches(['\\', '/']))
    }
    #[cfg(not(target_os = "windows"))]
    {
        left == right
    }
}

fn path_starts_with(path: &Path, base: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
        let base = base
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase();
        path == base || path.starts_with(&format!("{base}\\"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.starts_with(base)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mnemora-storage-{label}-{}", Uuid::new_v4()))
    }

    fn stage_pending_migration(config: &Path, pending: &mut PendingMigration) {
        fs::create_dir_all(&pending.staging).unwrap();
        for entry in &pending.entries {
            copy_tree(&pending.source.join(entry), &pending.staging.join(entry)).unwrap();
        }
        write_json_atomic(
            &pending.staging.join(STORAGE_MARKER_FILE_NAME),
            &StorageMarker {
                version: STORAGE_MARKER_VERSION,
                application: "com.mnemora.app".to_string(),
                created_at: now_millis(),
            },
        )
        .unwrap();
        verify_migration_copy(pending).unwrap();
        pending.phase = MigrationPhase::Committing;
        write_json_atomic(&config.join(MIGRATION_FILE_NAME), pending).unwrap();
        commit_install(pending).unwrap();
    }

    #[test]
    fn defaults_to_the_platform_data_directory() {
        let root = test_root("default");
        let config = root.join("config");
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        let manager = StorageManager::bootstrap(config, data.clone()).unwrap();
        assert!(manager.is_available());
        assert!(paths_equal(manager.current_data_dir(), &data));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovers_location_metadata_from_the_atomic_backup() {
        let root = test_root("location-backup");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&custom).unwrap();
        write_json_atomic(
            &custom.join(STORAGE_MARKER_FILE_NAME),
            &StorageMarker {
                version: STORAGE_MARKER_VERSION,
                application: "com.mnemora.app".to_string(),
                created_at: now_millis(),
            },
        )
        .unwrap();
        fs::create_dir_all(&config).unwrap();
        write_json_atomic(
            &config.join(LOCATION_FILE_NAME).with_extension("json.bak"),
            &StorageLocationFile {
                version: STORAGE_LOCATION_VERSION,
                custom_data_dir: Some(custom.clone()),
            },
        )
        .unwrap();

        let manager = StorageManager::bootstrap(config, data).unwrap();

        assert!(manager.is_available());
        assert!(paths_equal(manager.current_data_dir(), &custom));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_managed_data_and_keeps_the_source_as_a_backup() {
        let root = test_root("custom");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(data.join("conversations")).unwrap();
        fs::write(
            data.join("conversations").join("conv.json"),
            b"conversation",
        )
        .unwrap();
        fs::create_dir_all(data.join("library/notes/note-1/attachments")).unwrap();
        fs::write(data.join("library/notes/note-1/note.md"), b"# note").unwrap();
        fs::write(
            data.join("library/notes/note-1/attachments/chart.png"),
            b"chart",
        )
        .unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let migrated = StorageManager::bootstrap(config, data.clone()).unwrap();

        assert!(migrated.is_available());
        assert!(paths_equal(migrated.current_data_dir(), &custom));
        assert_eq!(
            fs::read(custom.join("conversations").join("conv.json")).unwrap(),
            b"conversation"
        );
        assert!(custom.join(STORAGE_MARKER_FILE_NAME).is_file());
        assert!(data.join("conversations").join("conv.json").is_file());
        assert_eq!(
            fs::read(custom.join("library/notes/note-1/note.md")).unwrap(),
            b"# note"
        );
        assert_eq!(
            fs::read(custom.join("library/notes/note-1/attachments/chart.png")).unwrap(),
            b"chart"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restores_custom_data_to_the_default_directory_without_removing_config_files() {
        let root = test_root("restore");
        let config = root.join("platform");
        let data = config.clone();
        let custom = root.join("custom");
        fs::create_dir_all(data.join("conversations")).unwrap();
        fs::write(data.join("app-settings.json"), b"settings").unwrap();
        fs::write(data.join("conversations").join("conv.json"), b"old").unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let migrated = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        fs::write(custom.join("conversations").join("conv.json"), b"new").unwrap();
        migrated.prepare_migration(data.clone()).unwrap();
        let restored = StorageManager::bootstrap(config, data.clone()).unwrap();

        assert!(paths_equal(restored.current_data_dir(), &data));
        assert_eq!(
            fs::read(data.join("app-settings.json")).unwrap(),
            b"settings"
        );
        assert_eq!(
            fs::read(data.join("conversations").join("conv.json")).unwrap(),
            b"new"
        );
        assert!(custom.join("conversations").join("conv.json").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_non_empty_custom_destination() {
        let root = test_root("occupied");
        let config = root.join("config");
        let data = root.join("data");
        let occupied = root.join("occupied");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join("unrelated.txt"), b"unrelated").unwrap();
        let manager = StorageManager::bootstrap(config, data).unwrap();
        assert!(manager.prepare_migration(occupied).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_an_empty_data_directory() {
        let root = test_root("empty");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(&data).unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let migrated = StorageManager::bootstrap(config, data).unwrap();

        assert!(migrated.is_available());
        assert!(paths_equal(migrated.current_data_dir(), &custom));
        assert!(custom.join(STORAGE_MARKER_FILE_NAME).is_file());
        assert_eq!(migrated.status().unwrap().total_bytes, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_migrated_sqlite_database_that_fails_integrity_check() {
        let root = test_root("invalid-sqlite");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(data.join("library")).unwrap();
        fs::write(
            data.join("library").join("library.sqlite3"),
            b"not a sqlite database",
        )
        .unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let recovered = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        let status = recovered.status().unwrap();

        assert!(recovered.is_available());
        assert!(paths_equal(recovered.current_data_dir(), &data));
        assert!(!custom.exists());
        assert!(status
            .last_migration
            .and_then(|result| result.error)
            .is_some_and(|error| error.contains("database")));
        assert!(!config.join(MIGRATION_FILE_NAME).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovers_when_commit_finished_before_the_phase_was_persisted() {
        let root = test_root("commit-recovery");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(data.join("conversations")).unwrap();
        fs::create_dir_all(data.join("memory")).unwrap();
        fs::write(
            data.join("conversations").join("conv.json"),
            b"conversation",
        )
        .unwrap();
        fs::write(data.join("memory").join("L1.md"), b"memory").unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let mut pending: PendingMigration = read_json(&config.join(MIGRATION_FILE_NAME)).unwrap();
        stage_pending_migration(&config, &mut pending);
        // Simulate power loss after every entry was installed while the durable phase is still
        // `Committing`. The next launch must roll back the uncertain install and retry from source.
        let recovered = StorageManager::bootstrap(config, data).unwrap();

        assert!(recovered.is_available());
        assert!(paths_equal(recovered.current_data_dir(), &custom));
        assert_eq!(
            fs::read(custom.join("conversations").join("conv.json")).unwrap(),
            b"conversation"
        );
        assert_eq!(
            fs::read(custom.join("memory").join("L1.md")).unwrap(),
            b"memory"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completes_location_switch_after_install_was_persisted() {
        let root = test_root("install-complete");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(data.join("conversations")).unwrap();
        fs::write(
            data.join("conversations").join("conv.json"),
            b"conversation",
        )
        .unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let mut pending: PendingMigration = read_json(&config.join(MIGRATION_FILE_NAME)).unwrap();
        stage_pending_migration(&config, &mut pending);
        pending.phase = MigrationPhase::InstallComplete;
        write_json_atomic(&config.join(MIGRATION_FILE_NAME), &pending).unwrap();

        let recovered = StorageManager::bootstrap(config.clone(), data).unwrap();

        assert!(recovered.is_available());
        assert!(paths_equal(recovered.current_data_dir(), &custom));
        assert!(!config.join(MIGRATION_FILE_NAME).exists());
        assert_eq!(
            fs::read(custom.join("conversations").join("conv.json")).unwrap(),
            b"conversation"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn blocks_repository_storage_when_the_custom_directory_disappears() {
        let root = test_root("missing-custom");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(data.join("conversations")).unwrap();
        fs::write(
            data.join("conversations").join("conv.json"),
            b"conversation",
        )
        .unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let migrated = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        assert!(migrated.is_available());
        fs::remove_dir_all(&custom).unwrap();

        let unavailable = StorageManager::bootstrap(config, data).unwrap();

        assert!(!unavailable.is_available());
        assert!(paths_equal(unavailable.current_data_dir(), &custom));
        assert!(unavailable.runtime_data_dir().is_file());
        assert!(unavailable.status().unwrap().availability_error.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn blocks_repository_storage_when_the_custom_marker_is_corrupted() {
        let root = test_root("corrupt-marker");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        fs::create_dir_all(&data).unwrap();

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        let migrated = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        assert!(migrated.is_available());
        fs::write(custom.join(STORAGE_MARKER_FILE_NAME), b"{broken").unwrap();

        let unavailable = StorageManager::bootstrap(config, data).unwrap();

        assert!(!unavailable.is_available());
        assert!(paths_equal(unavailable.current_data_dir(), &custom));
        assert!(unavailable.runtime_data_dir().is_file());
        let _ = fs::remove_dir_all(root);
    }

    /// 在 WAL 模式下写入并**不主动检查点**，让最近的已提交事务只存在于 `-wal` 里，
    /// 然后走一次完整迁移，断言数据在目标库中完好。
    ///
    /// 这正是「只开 WAL 不改备份」会产出的坏备份：主库文件里没有那几行。
    #[test]
    fn migration_preserves_transactions_that_still_live_in_the_wal() {
        let root = test_root("wal-migration");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        let library_dir = data.join("library");
        fs::create_dir_all(&library_dir).unwrap();

        let database = library_dir.join("library.sqlite3");
        // 连接必须**跨过迁移继续存活**。SQLite 在最后一条连接关闭时会自动做检查点并
        // 删掉 `-wal`，那样「已提交事务只活在 WAL 里」这个前提就不成立了，测试也就
        // 证伪不了「不做检查点会丢事务」。
        let writer = Connection::open(&database).unwrap();
        let mode: String = writer
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal", "测试前提是库确实在 WAL 模式");
        writer
            .execute_batch(
                "CREATE TABLE probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO probe (value) VALUES ('committed-into-wal');",
            )
            .unwrap();
        // 刻意不做检查点：让事务停留在 `-wal` 里。
        assert!(
            database.with_file_name("library.sqlite3-wal").exists(),
            "前提校验：此刻 -wal 必须存在，否则这个测试没有在测它想测的东西"
        );

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        // `prepare_migration` 只写迁移日志，真正执行迁移的是下一次 bootstrap。
        let migrated = StorageManager::bootstrap(config, data.clone()).unwrap();
        assert!(migrated.is_available());
        drop(writer);

        let migrated_database = custom.join("library").join("library.sqlite3");
        assert!(migrated_database.is_file(), "迁移后的库文件必须存在");
        let connection = Connection::open(&migrated_database).unwrap();
        let value: String = connection
            .query_row("SELECT value FROM probe", [], |row| row.get(0))
            .expect("只存在于 WAL 里的已提交事务必须随迁移一起到达目标库");
        assert_eq!(value, "committed-into-wal");
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    /// 装进目标目录的库不应带着伴生文件。
    ///
    /// 陈旧的 `-wal` 被带过去后，下次打开会被 SQLite 重放，静默把刚装好的数据
    /// 回滚 —— 这种失效完全没有报错。
    #[test]
    fn migrated_databases_carry_no_stale_sqlite_sidecars() {
        let root = test_root("wal-sidecars");
        let config = root.join("config");
        let data = root.join("data");
        let custom = root.join("custom");
        let library_dir = data.join("library");
        fs::create_dir_all(&library_dir).unwrap();

        let database = library_dir.join("library.sqlite3");
        // 同上：连接跨过迁移存活，源目录才会真的带着 `-wal` 进入复制环节。
        let writer = Connection::open(&database).unwrap();
        writer
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap();
        writer
            .execute_batch("CREATE TABLE probe (id INTEGER PRIMARY KEY);")
            .unwrap();
        assert!(
            database.with_file_name("library.sqlite3-wal").exists(),
            "前提校验：源目录此刻必须有 -wal 可供带走"
        );

        let manager = StorageManager::bootstrap(config.clone(), data.clone()).unwrap();
        manager.prepare_migration(custom.clone()).unwrap();
        // 真正执行迁移的是这一次 bootstrap，不是 prepare_migration。
        let migrated = StorageManager::bootstrap(config, data.clone()).unwrap();
        assert!(migrated.is_available());
        drop(writer);

        let migrated = custom.join("library").join("library.sqlite3");
        assert!(migrated.is_file(), "迁移后的库文件必须存在");
        for suffix in SQLITE_SIDECAR_SUFFIXES {
            let sidecar = migrated.with_file_name(format!("library.sqlite3{suffix}"));
            assert!(
                !sidecar.exists(),
                "目标目录残留了 {suffix}，下次打开会重放它并回滚已恢复的数据"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sidecar_removal_covers_every_known_suffix_and_tolerates_absence() {
        let root = test_root("sidecar-removal");
        fs::create_dir_all(&root).unwrap();
        let database = root.join("library.sqlite3");
        fs::write(&database, b"main").unwrap();
        for suffix in SQLITE_SIDECAR_SUFFIXES {
            fs::write(root.join(format!("library.sqlite3{suffix}")), b"sidecar").unwrap();
        }

        remove_sqlite_sidecars(&database).unwrap();

        for suffix in SQLITE_SIDECAR_SUFFIXES {
            assert!(!root.join(format!("library.sqlite3{suffix}")).exists());
        }
        assert!(database.is_file(), "只清伴生文件，主库不能动");
        // 再来一次：文件已经不存在，不应报错。
        remove_sqlite_sidecars(&database).unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
