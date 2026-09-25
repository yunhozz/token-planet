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

#[derive(Clone)]
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
    pub codex_source: SourceHealth,
    pub claude_code_source: SourceHealth,
    pub confirmed_subtotal: Option<u64>,
    pub complete_total: Option<u64>,
    pub scanned_at_utc: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceHealth {
    Ready,
    NotFound,
    PermissionDenied,
    UnsupportedFormat,
    UsageUnavailable,
    Partial,
    UserDisabled,
}

pub fn scan_sources(config: &SourceConfig, ledger: &mut Ledger) -> Result<ScanSummary, ScanError> {
    if ledger.timezone != config.timezone {
        return Err(ScanError::TimezoneMismatch);
    }
    let codex_enabled = ledger.agent_enabled(Agent::Codex)?;
    let claude_enabled = ledger.agent_enabled(Agent::ClaudeCode)?;
    if codex_enabled {
        ledger.ensure_source_root(
            Agent::Codex,
            &hex_hash(config.codex_root.as_os_str().as_encoded_bytes()),
        )?;
    }
    if claude_enabled {
        ledger.ensure_source_root(
            Agent::ClaudeCode,
            &hex_hash(config.claude_root.as_os_str().as_encoded_bytes()),
        )?;
    }
    let codex_source = if codex_enabled {
        scan_root(&config.codex_root, Agent::Codex, ledger)?
    } else {
        SourceHealth::UserDisabled
    };
    let claude_code_source = if claude_enabled {
        scan_root(&config.claude_root, Agent::ClaudeCode, ledger)?
    } else {
        SourceHealth::UserDisabled
    };
    let codex = summarized_usage(
        ledger,
        Agent::Codex,
        codex_source == SourceHealth::Ready,
        codex_enabled,
    )?;
    let claude_code = summarized_usage(
        ledger,
        Agent::ClaudeCode,
        claude_code_source == SourceHealth::Ready,
        claude_enabled,
    )?;
    let source_health = |scan_health, coverage| {
        if scan_health != SourceHealth::Ready {
            return scan_health;
        }
        match coverage {
            UsageCoverage::Complete => SourceHealth::Ready,
            UsageCoverage::Partial => SourceHealth::Partial,
            UsageCoverage::Unsupported => SourceHealth::UnsupportedFormat,
            UsageCoverage::Unavailable => SourceHealth::UsageUnavailable,
            UsageCoverage::UserDisabled => SourceHealth::UserDisabled,
        }
    };
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
    let codex_source = source_health(codex_source, codex.coverage);
    let claude_code_source = source_health(claude_code_source, claude_code.coverage);
    Ok(ScanSummary {
        codex,
        claude_code,
        codex_source,
        claude_code_source,
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

fn scan_root(root: &Path, agent: Agent, ledger: &mut Ledger) -> Result<SourceHealth, ScanError> {
    match fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(SourceHealth::NotFound),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceHealth::NotFound)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok(SourceHealth::PermissionDenied)
        }
        Err(_) => return Ok(SourceHealth::UsageUnavailable),
    }
    let mut health = SourceHealth::Ready;
    let mut found_file = false;
    for entry in WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(entry)
                if entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|e| e == "jsonl") =>
            {
                found_file = true;
                match scan_file(entry.path(), agent, ledger) {
                    Ok(()) => {}
                    Err(ScanError::SourcePermission) => health = SourceHealth::PermissionDenied,
                    Err(ScanError::SourceIo) => health = SourceHealth::UsageUnavailable,
                    Err(error) => return Err(error),
                }
            }
            Ok(_) => {}
            Err(error)
                if error
                    .io_error()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied) =>
            {
                health = SourceHealth::PermissionDenied
            }
            Err(_) => health = SourceHealth::UsageUnavailable,
        }
    }
    Ok(if !found_file && health == SourceHealth::Ready {
        SourceHealth::UsageUnavailable
    } else {
        health
    })
}

fn scan_file(path: &Path, agent: Agent, ledger: &mut Ledger) -> Result<(), ScanError> {
    let file = File::open(path).map_err(source_io_error)?;
    let meta = file.metadata().map_err(source_io_error)?;
    let source_id = format!(
        "{}:{}",
        agent_name(agent),
        hex_hash(path.to_string_lossy().as_bytes())
    );
    let version = match agent {
        Agent::Codex => codex::PARSER_VERSION,
        Agent::ClaudeCode => claude_code::PARSER_VERSION,
    };
    let prior: Option<(String, i64, i64, Option<i64>, String)> = ledger.connection.query_row(
        "SELECT file_fingerprint,byte_offset,parser_version,last_snapshot_total,status FROM source_checkpoint WHERE source_id=?1",
        [&source_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
    ).optional()?;
    let resume = match prior.as_ref() {
        Some(previous)
            if previous.1 >= 0
                && previous.2 == i64::from(version)
                && (previous.1 as u64) <= meta.len()
                && previous.0 == prefix_fingerprint(path, previous.1 as u64)? =>
        {
            Some(previous)
        }
        _ => None,
    };
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
    if prior.is_some() && resume.is_none() {
        tx.execute("DELETE FROM usage_record WHERE source_id=?1", [&source_id])?;
    }
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
    drop(reader);
    let fingerprint = prefix_fingerprint(path, next_offset)?;
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

fn source_io_error(error: std::io::Error) -> ScanError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ScanError::SourcePermission
    } else {
        ScanError::SourceIo
    }
}

fn prefix_fingerprint(path: &Path, length: u64) -> Result<String, ScanError> {
    let mut file = File::open(path).map_err(source_io_error)?;
    let mut remaining = length;
    let mut buffer = [0_u8; 8192];
    let mut digest = Sha256::new();
    while remaining > 0 {
        let chunk = remaining.min(buffer.len() as u64) as usize;
        let amount = file.read(&mut buffer[..chunk]).map_err(source_io_error)?;
        if amount == 0 {
            return Err(ScanError::SourceIo);
        }
        digest.update(&buffer[..amount]);
        remaining -= amount as u64;
    }
    Ok(format!("v2:{}", hex_digest(&digest.finalize())))
}

fn hex_hash(input: &[u8]) -> String {
    hex_digest(&Sha256::digest(input))
}

fn hex_digest(input: &[u8]) -> String {
    input.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_roots, scan_sources, source_io_error, RootOptions, SourceConfig, SourceHealth,
    };
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
    fn missing_source_and_empty_readable_source_have_distinct_health() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: Seoul,
        };
        let summary = scan_sources(&config, &mut ledger).unwrap();
        assert_eq!(summary.codex_source, SourceHealth::UsageUnavailable);
        assert_eq!(summary.claude_code_source, SourceHealth::NotFound);
        assert_eq!(summary.claude_code.total_tokens, None);
    }

    #[test]
    fn changing_source_folder_replaces_prior_folder_totals() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        for (root, session, total) in [(&first, "s1", 42), (&second, "s2", 19)] {
            fs::write(
                root.join("session.jsonl"),
                format!("{{\"timestamp\":\"2026-09-25T00:00:00Z\",\"type\":\"token_usage_record\",\"payload\":{{\"session_id\":\"{session}\",\"response_id\":\"r1\",\"usage\":{{\"total_tokens\":{total}}}}}}}\n"),
            )
            .unwrap();
        }
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        let mut config = SourceConfig {
            codex_root: first,
            claude_root: temp.path().join("missing"),
            timezone: Seoul,
        };
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            Some(42)
        );
        config.codex_root = second;
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            Some(19)
        );
        assert_eq!(ledger.daily_known_totals().unwrap(), vec![19]);
    }

    #[test]
    fn rewriting_same_file_replaces_stale_record_even_when_prefix_is_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let source = codex.join("session.jsonl");
        let row = |total| {
            format!("{{\"timestamp\":\"2026-09-25T00:00:00Z\",\"type\":\"token_usage_record\",\"payload\":{{\"session_id\":\"s1\",\"response_id\":\"r1\",\"usage\":{{\"total_tokens\":{total}}}}}}}\n")
        };
        fs::write(&source, row(42)).unwrap();
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
            Some(42)
        );
        fs::write(&source, row(19)).unwrap();
        assert_eq!(
            scan_sources(&config, &mut ledger)
                .unwrap()
                .codex
                .total_tokens,
            Some(19)
        );
    }

    #[test]
    fn denied_file_access_is_distinct_from_other_io_failures() {
        assert_eq!(
            source_io_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            crate::storage::ledger::ScanError::SourcePermission
        );
        assert_eq!(
            source_io_error(std::io::Error::from(std::io::ErrorKind::Other)),
            crate::storage::ledger::ScanError::SourceIo
        );
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
            scan_sources(&config, &mut ledger).unwrap().codex_source,
            SourceHealth::UnsupportedFormat
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
