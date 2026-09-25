use std::{
    env,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::collectors::{claude_code, codex, RecordKind};
use crate::domain::usage::{known_subtotal, Agent, TokenUsage, UsageCoverage};
use crate::storage::ledger::{agent_name, insert_record, rebuild_daily, Ledger, ScanError};

pub struct RootOptions {
    pub home: PathBuf,
    pub codex_home: Option<PathBuf>,
    pub claude_config_dir: Option<PathBuf>,
    pub codex_custom: Option<PathBuf>,
    pub claude_custom: Option<PathBuf>,
    pub timezone: Tz,
}

impl RootOptions {
    pub fn from_env(
        codex_custom: Option<PathBuf>,
        claude_custom: Option<PathBuf>,
        timezone: Tz,
    ) -> Option<Self> {
        let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        Some(Self {
            home: home.into(),
            codex_home: env::var_os("CODEX_HOME").map(PathBuf::from),
            claude_config_dir: env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
            codex_custom,
            claude_custom,
            timezone,
        })
    }
}

pub struct SourceConfig {
    pub codex_root: PathBuf,
    pub claude_root: PathBuf,
    pub timezone: Tz,
}

pub fn resolve_roots(options: &RootOptions) -> SourceConfig {
    SourceConfig {
        codex_root: options.codex_custom.clone().unwrap_or_else(|| {
            options
                .codex_home
                .clone()
                .unwrap_or_else(|| options.home.join(".codex"))
                .join("sessions")
        }),
        claude_root: options.claude_custom.clone().unwrap_or_else(|| {
            options
                .claude_config_dir
                .clone()
                .unwrap_or_else(|| options.home.join(".claude"))
                .join("projects")
        }),
        timezone: options.timezone,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanSummary {
    pub codex: TokenUsage,
    pub claude_code: TokenUsage,
    pub confirmed_subtotal: Option<u64>,
    pub complete_total: Option<u64>,
    pub scanned_at_utc: DateTime<Utc>,
}

pub fn scan_sources(config: &SourceConfig, ledger: &mut Ledger) -> Result<ScanSummary, ScanError> {
    if ledger.timezone != config.timezone {
        return Err(ScanError::TimezoneMismatch);
    }
    let codex_enabled = ledger.agent_enabled(Agent::Codex)?;
    let claude_enabled = ledger.agent_enabled(Agent::ClaudeCode)?;
    let codex_ok = if codex_enabled {
        scan_root(&config.codex_root, Agent::Codex, ledger)?
    } else {
        false
    };
    let claude_ok = if claude_enabled {
        scan_root(&config.claude_root, Agent::ClaudeCode, ledger)?
    } else {
        false
    };
    let codex = summarized_usage(ledger, Agent::Codex, codex_ok, codex_enabled)?;
    let claude_code = summarized_usage(ledger, Agent::ClaudeCode, claude_ok, claude_enabled)?;
    let confirmed_subtotal = known_subtotal(&[codex.clone(), claude_code.clone()]);
    let complete_total = if [codex.coverage, claude_code.coverage]
        .iter()
        .all(|coverage| {
            matches!(
                coverage,
                UsageCoverage::Complete | UsageCoverage::UserDisabled
            )
        }) {
        confirmed_subtotal
    } else {
        None
    };
    Ok(ScanSummary {
        codex,
        claude_code,
        confirmed_subtotal,
        complete_total,
        scanned_at_utc: Utc::now(),
    })
}

fn summarized_usage(
    ledger: &Ledger,
    agent: Agent,
    root_ok: bool,
    enabled: bool,
) -> Result<TokenUsage, ScanError> {
    if !enabled {
        return Ok(TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: None,
            coverage: UsageCoverage::UserDisabled,
        });
    }
    let mut usage = ledger.all_time_usage(agent)?;
    let invalid_count: i64 = ledger.connection.query_row(
        "SELECT COUNT(*) FROM source_checkpoint WHERE source_id LIKE ?1 AND status != 'complete'",
        [format!("{}:%", agent_name(agent))],
        |row| row.get(0),
    )?;
    if !root_ok || invalid_count > 0 {
        usage.coverage = if usage.total_tokens.is_some() {
            UsageCoverage::Partial
        } else if invalid_count > 0 {
            UsageCoverage::Unsupported
        } else {
            UsageCoverage::Unavailable
        };
    }
    Ok(usage)
}

fn scan_root(root: &Path, agent: Agent, ledger: &mut Ledger) -> Result<bool, ScanError> {
    if !root.is_dir() {
        return Ok(false);
    }
    let mut readable = true;
    for entry in WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(entry)
                if entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|e| e == "jsonl") =>
            {
                if scan_file(entry.path(), agent, ledger).is_err() {
                    readable = false;
                }
            }
            Ok(_) => {}
            Err(_) => readable = false,
        }
    }
    Ok(readable)
}

fn scan_file(path: &Path, agent: Agent, ledger: &mut Ledger) -> Result<(), ScanError> {
    let file = File::open(path).map_err(|_| ScanError::SourceIo)?;
    let meta = file.metadata().map_err(|_| ScanError::SourceIo)?;
    let source_id = format!(
        "{}:{}",
        agent_name(agent),
        hex_hash(path.to_string_lossy().as_bytes())
    );
    let fingerprint = file_fingerprint(path, &meta)?;
    let version = match agent {
        Agent::Codex => codex::PARSER_VERSION,
        Agent::ClaudeCode => claude_code::PARSER_VERSION,
    };
    let prior: Option<(String, i64, i64, Option<i64>, String)> = ledger.connection.query_row(
        "SELECT file_fingerprint,byte_offset,parser_version,last_snapshot_total,status FROM source_checkpoint WHERE source_id=?1",
        [&source_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
    ).optional()?;
    let resume = prior
        .as_ref()
        .filter(|(fingerprint_old, offset, version_old, _, _)| {
            fingerprint_old == &fingerprint
                && *version_old == i64::from(version)
                && (*offset as u64) <= meta.len()
        });
    let start = resume
        .map(|(_, offset, _, _, _)| *offset as u64)
        .unwrap_or(0);
    if start == meta.len() {
        return Ok(());
    }
    let mut previous_snapshot = resume.and_then(|(_, _, _, total, _)| *total).unwrap_or(0);
    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(start))
        .map_err(|_| ScanError::SourceIo)?;
    let mut next_offset = start;
    let mut status = resume
        .map(|(_, _, _, _, status)| status.as_str())
        .unwrap_or("complete");
    let tx = ledger.connection.transaction()?;
    loop {
        let mut bytes = Vec::new();
        let length = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|_| ScanError::SourceIo)?;
        if length == 0 || bytes.last() != Some(&b'\n') {
            break;
        }
        next_offset += length as u64;
        let line = match std::str::from_utf8(&bytes) {
            Ok(line) => line.trim_end_matches(['\r', '\n']),
            Err(_) => {
                status = "unsupported";
                continue;
            }
        };
        if line.is_empty() {
            continue;
        }
        let parsed = match agent {
            Agent::Codex => codex::parse_line(line),
            Agent::ClaudeCode => claude_code::parse_line(line),
        };
        match parsed {
            Ok(Some(mut record)) => {
                if record.kind == RecordKind::CumulativeSnapshot {
                    if let Some(total) = record.usage.total_tokens {
                        let current = i64::try_from(total).map_err(|_| ScanError::InvalidCount)?;
                        record.usage.total_tokens = Some(if current >= previous_snapshot {
                            (current - previous_snapshot) as u64
                        } else {
                            total
                        });
                        previous_snapshot = current;
                    }
                }
                insert_record(&tx, &record, &source_id, ledger.timezone)?;
            }
            Ok(None) => {}
            Err(_) => status = "unsupported",
        }
    }
    tx.execute("INSERT INTO source_checkpoint(source_id,file_fingerprint,byte_offset,parser_version,last_snapshot_total,status)
        VALUES (?1,?2,?3,?4,?5,?6)
        ON CONFLICT(source_id) DO UPDATE SET file_fingerprint=excluded.file_fingerprint,
        byte_offset=excluded.byte_offset,parser_version=excluded.parser_version,
        last_snapshot_total=excluded.last_snapshot_total,status=excluded.status",
        params![source_id, fingerprint, i64::try_from(next_offset).map_err(|_| ScanError::SourceIo)?,
            version, previous_snapshot, status])?;
    rebuild_daily(&tx)?;
    tx.commit()?;
    Ok(())
}

fn file_fingerprint(path: &Path, metadata: &fs::Metadata) -> Result<String, ScanError> {
    let mut data = path.to_string_lossy().as_bytes().to_vec();
    let mut file = File::open(path).map_err(|_| ScanError::SourceIo)?;
    let mut prefix = [0_u8; 64];
    let length = file.read(&mut prefix).map_err(|_| ScanError::SourceIo)?;
    data.extend_from_slice(&prefix[..length]);
    if let Ok(created) = metadata.created() {
        if let Ok(duration) = created.duration_since(std::time::UNIX_EPOCH) {
            data.extend_from_slice(&duration.as_nanos().to_le_bytes());
        }
    }
    Ok(hex_hash(&data))
}

fn hex_hash(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{resolve_roots, scan_sources, RootOptions, SourceConfig};
    use crate::storage::ledger::Ledger;
    use chrono_tz::Asia::Seoul;
    use std::{fs, path::PathBuf};

    #[test]
    fn default_override_and_custom_source_roots_are_resolved() {
        let home = PathBuf::from("/home/example");
        let defaults = resolve_roots(&RootOptions {
            home: home.clone(),
            codex_home: None,
            claude_config_dir: None,
            codex_custom: None,
            claude_custom: None,
            timezone: Seoul,
        });
        assert_eq!(defaults.codex_root, home.join(".codex/sessions"));
        assert_eq!(defaults.claude_root, home.join(".claude/projects"));
        let changed = resolve_roots(&RootOptions {
            home,
            codex_home: Some(PathBuf::from("/agent")),
            claude_config_dir: Some(PathBuf::from("/claude")),
            codex_custom: Some(PathBuf::from("/chosen/codex")),
            claude_custom: None,
            timezone: Seoul,
        });
        assert_eq!(changed.codex_root, PathBuf::from("/chosen/codex"));
        assert_eq!(changed.claude_root, PathBuf::from("/claude/projects"));
    }

    #[test]
    fn partial_final_line_waits_for_newline_and_restart_does_not_recount() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let source = codex.join("session.jsonl");
        let row = r#"{"timestamp":"2026-09-25T00:00:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#;
        fs::write(&source, &row[..row.len() - 1]).unwrap();
        let db = temp.path().join("ledger.db");
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: Seoul,
        };
        let mut ledger = Ledger::open(&db, Seoul).unwrap();
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            None
        );
        fs::write(&source, format!("{row}\n")).unwrap();
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            Some(42)
        );
        drop(ledger);
        let mut reopened = Ledger::open(&db, Seoul).unwrap();
        assert_eq!(
            scan_sources(&config, &mut reopened)
                .unwrap()
                .codex
                .total_tokens,
            Some(42)
        );
    }

    #[test]
    fn unsupported_line_remains_reported_after_unchanged_rescan() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        fs::write(codex.join("session.jsonl"), "{broken\n").unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: Seoul,
        };
        assert_eq!(
            scan_sources(&config, &mut ledger).unwrap().codex.coverage,
            crate::domain::usage::UsageCoverage::Unsupported
        );
        assert_eq!(
            scan_sources(&config, &mut ledger).unwrap().codex.coverage,
            crate::domain::usage::UsageCoverage::Unsupported
        );
    }

    #[test]
    fn scanning_never_changes_source_file() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let source = codex.join("session.jsonl");
        let contents = b"{\"timestamp\":\"2026-09-25T00:00:00Z\",\"type\":\"token_usage_record\",\"payload\":{\"session_id\":\"s1\",\"response_id\":\"r1\",\"usage\":{\"total_tokens\":42}}}\n";
        fs::write(&source, contents).unwrap();
        let modified = fs::metadata(&source).unwrap().modified().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: Seoul,
        };
        scan_sources(&config, &mut ledger).unwrap();
        assert_eq!(fs::read(&source).unwrap(), contents);
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn cumulative_fallback_never_adds_existing_total_twice() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        fs::write(codex.join("session.jsonl"), concat!(
            "{\"timestamp\":\"2026-09-25T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":42}}}}\n",
            "{\"timestamp\":\"2026-09-25T00:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":42}}}}\n",
            "{\"timestamp\":\"2026-09-25T00:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":50}}}}\n"
        )).unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: Seoul,
        };
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            Some(50)
        );
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            Some(50)
        );
    }
}
