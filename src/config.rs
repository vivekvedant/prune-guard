use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Configuration layer for phase 1.
///
/// Why this exists:
/// - keep parsing/validation isolated from business logic
/// - expose one typed `Config` used by all runtime components
/// - enforce fail-closed defaults before later phases are implemented
///
/// Note: this intentionally parses a constrained TOML subset so behavior stays
/// predictable and testable in early phases.
/// Safety-first daemon configuration.
///
/// This module intentionally keeps parsing conservative:
/// - unsupported keys are rejected
/// - invalid threshold relationships are rejected
/// - `dry_run` defaults to `false`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Daemon scheduler interval (seconds).
    pub interval_secs: u64,
    /// Start cleanup at/above this usage percentage.
    pub high_watermark_percent: u8,
    /// Stop cleanup once usage falls below this percentage.
    pub target_watermark_percent: u8,
    /// Skip artifacts newer than this age.
    pub min_unused_age_days: u64,
    /// Maximum allowed deletion volume per run (GB).
    pub max_delete_per_run_gb: u64,
    /// Runtime execution mode flag (`true` means simulate-only).
    pub dry_run: bool,
    /// Opt-in compatibility mode for runtimes that omit image labels metadata.
    pub allow_missing_image_labels: bool,
    /// Enabled backend identifiers (for example: docker, podman).
    pub enabled_backends: Vec<String>,
    /// Explicitly protected images.
    pub protected_images: Vec<String>,
    /// Explicitly protected volumes.
    pub protected_volumes: Vec<String>,
    /// Labels that imply protection.
    pub protected_labels: Vec<String>,
    /// Optional Docker daemon endpoint override used for all docker CLI calls.
    pub docker_host: Option<String>,
    /// Optional Docker CLI context override used for all docker CLI calls.
    pub docker_context: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval_secs: 900,
            high_watermark_percent: 85,
            target_watermark_percent: 70,
            min_unused_age_days: 30,
            max_delete_per_run_gb: 10,
            dry_run: false,
            allow_missing_image_labels: false,
            enabled_backends: vec!["docker".to_string()],
            protected_images: Vec::new(),
            protected_volumes: Vec::new(),
            protected_labels: Vec::new(),
            docker_host: None,
            docker_context: None,
        }
    }
}

impl Config {
    /// Parse config from a TOML string.
    pub fn parse_str(input: &str) -> Result<Self, ConfigError> {
        let entries = parse_toml_subset(input)?;
        Self::from_entries(entries)
    }

    /// Parse config from any reader (`File`, `Cursor`, etc.).
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self, ConfigError> {
        let mut contents = String::new();
        reader.read_to_string(&mut contents)?;
        Self::parse_str(&contents)
    }

    /// Parse config from file path.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path)?;
        Self::parse_str(&contents)
    }

    fn from_entries(entries: BTreeMap<String, TomlValue>) -> Result<Self, ConfigError> {
        let mut config = Self::default();

        config.interval_secs = take_u64(
            &entries,
            &["interval_secs", "runtime.interval_secs"],
            config.interval_secs,
            "interval_secs",
        )?;
        config.high_watermark_percent = take_u8(
            &entries,
            &[
                "high_watermark_percent",
                "thresholds.high_watermark_percent",
                "watermarks.high_watermark_percent",
            ],
            config.high_watermark_percent,
            "high_watermark_percent",
        )?;
        config.target_watermark_percent = take_u8(
            &entries,
            &[
                "target_watermark_percent",
                "thresholds.target_watermark_percent",
                "watermarks.target_watermark_percent",
            ],
            config.target_watermark_percent,
            "target_watermark_percent",
        )?;
        config.min_unused_age_days = take_u64(
            &entries,
            &["min_unused_age_days", "cleanup.min_unused_age_days"],
            config.min_unused_age_days,
            "min_unused_age_days",
        )?;
        config.max_delete_per_run_gb = take_u64(
            &entries,
            &["max_delete_per_run_gb", "cleanup.max_delete_per_run_gb"],
            config.max_delete_per_run_gb,
            "max_delete_per_run_gb",
        )?;
        config.dry_run = take_bool(
            &entries,
            &["dry_run", "safety.dry_run", "runtime.dry_run"],
            config.dry_run,
            "dry_run",
        )?;
        config.allow_missing_image_labels = take_bool(
            &entries,
            &[
                "allow_missing_image_labels",
                "safety.allow_missing_image_labels",
            ],
            config.allow_missing_image_labels,
            "allow_missing_image_labels",
        )?;
        config.enabled_backends = take_string_array(
            &entries,
            &["enabled_backends", "backends.enabled_backends"],
            config.enabled_backends.clone(),
            "enabled_backends",
        )?;
        config.protected_images = take_string_array(
            &entries,
            &[
                "protected_images",
                "allowlists.protected_images",
                "safety.protected_images",
            ],
            config.protected_images.clone(),
            "protected_images",
        )?;
        config.protected_volumes = take_string_array(
            &entries,
            &[
                "protected_volumes",
                "allowlists.protected_volumes",
                "safety.protected_volumes",
            ],
            config.protected_volumes.clone(),
            "protected_volumes",
        )?;
        config.protected_labels = take_string_array(
            &entries,
            &[
                "protected_labels",
                "allowlists.protected_labels",
                "safety.protected_labels",
            ],
            config.protected_labels.clone(),
            "protected_labels",
        )?;
        config.docker_host = take_optional_string(
            &entries,
            &["docker_host", "docker.host"],
            config.docker_host.clone(),
            "docker_host",
        )?;
        config.docker_context = take_optional_string(
            &entries,
            &["docker_context", "docker.context"],
            config.docker_context.clone(),
            "docker_context",
        )?;

        ensure_no_unknown_keys(&entries)?;
        config.validate()?;
        Ok(config)
    }

    /// Validation intentionally encodes fail-closed guardrails that must hold
    /// before any cleanup logic is allowed to run.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.interval_secs == 0 {
            return Err(ConfigError::validation(
                "interval_secs must be greater than 0",
            ));
        }
        if self.high_watermark_percent > 100 {
            return Err(ConfigError::validation(
                "high_watermark_percent must be between 0 and 100",
            ));
        }
        if self.target_watermark_percent > 100 {
            return Err(ConfigError::validation(
                "target_watermark_percent must be between 0 and 100",
            ));
        }
        if self.target_watermark_percent >= self.high_watermark_percent {
            return Err(ConfigError::validation(
                "target_watermark_percent must be lower than high_watermark_percent",
            ));
        }
        if let Some(host) = &self.docker_host {
            if host.trim().is_empty() {
                return Err(ConfigError::validation("docker.host cannot be empty"));
            }
        }
        if let Some(context) = &self.docker_context {
            if context.trim().is_empty() {
                return Err(ConfigError::validation("docker.context cannot be empty"));
            }
        }
        if self.docker_host.is_some() && self.docker_context.is_some() {
            return Err(ConfigError::validation(
                "docker.host and docker.context cannot both be set",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TomlValue {
    Bool(bool),
    Integer(i64),
    String(String),
    Array(Vec<TomlValue>),
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse {
        line: Option<usize>,
        key: Option<String>,
        message: String,
    },
    Validation(String),
}

impl ConfigError {
    fn parse(line: usize, key: Option<String>, message: impl Into<String>) -> Self {
        Self::Parse {
            line: Some(line),
            key,
            message: message.into(),
        }
    }

    fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Parse { line, key, message } => match (line, key) {
                (Some(line), Some(key)) => {
                    write!(f, "parse error on line {line} for `{key}`: {message}")
                }
                (Some(line), None) => write!(f, "parse error on line {line}: {message}"),
                (None, Some(key)) => write!(f, "parse error for `{key}`: {message}"),
                (None, None) => write!(f, "parse error: {message}"),
            },
            Self::Validation(message) => write!(f, "validation error: {message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

fn parse_toml_subset(input: &str) -> Result<BTreeMap<String, TomlValue>, ConfigError> {
    let mut entries = BTreeMap::new();
    let mut current_section: Vec<String> = Vec::new();

    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw_line).trim().to_string();
        if line.is_empty() {
            continue;
        }

        // Section headers like `[runtime]` build key prefixes such as
        // `runtime.interval_secs`.
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(ConfigError::parse(
                    line_no,
                    None,
                    "unterminated section header",
                ));
            }
            let section = line[1..line.len() - 1].trim();
            if section.is_empty() {
                return Err(ConfigError::parse(line_no, None, "empty section header"));
            }
            current_section = section
                .split('.')
                .map(|part| part.trim().to_string())
                .collect();
            continue;
        }

        let (raw_key, raw_value) = line.split_once('=').ok_or_else(|| {
            ConfigError::parse(line_no, None, "expected `key = value` assignment")
        })?;
        let raw_key = raw_key.trim();
        let raw_value = raw_value.trim();
        if raw_key.is_empty() {
            return Err(ConfigError::parse(line_no, None, "empty key"));
        }

        let mut full_key = String::new();
        if !current_section.is_empty() {
            full_key.push_str(&current_section.join("."));
            full_key.push('.');
        }
        full_key.push_str(raw_key);

        // Parse literal into our internal value enum.
        let value = parse_value(raw_value, line_no)?;
        if entries.insert(full_key.clone(), value).is_some() {
            return Err(ConfigError::parse(
                line_no,
                Some(full_key),
                "duplicate key in TOML input",
            ));
        }
    }

    Ok(entries)
}

fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escaped = false;

    // Strip comments while preserving `#` inside quoted strings.
    for ch in line.chars() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '#' => break,
            _ => out.push(ch),
        }
    }

    out
}

fn parse_value(raw: &str, line_no: usize) -> Result<TomlValue, ConfigError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ConfigError::parse(line_no, None, "missing value"));
    }

    if raw.starts_with('"') {
        return Ok(TomlValue::String(parse_string(raw, line_no)?));
    }

    if raw.eq_ignore_ascii_case("true") {
        return Ok(TomlValue::Bool(true));
    }
    if raw.eq_ignore_ascii_case("false") {
        return Ok(TomlValue::Bool(false));
    }

    if raw.starts_with('[') {
        return Ok(TomlValue::Array(parse_array(raw, line_no)?));
    }

    let parsed = raw
        .parse::<i64>()
        .map_err(|_| ConfigError::parse(line_no, None, "unsupported TOML value"))?;
    Ok(TomlValue::Integer(parsed))
}

fn parse_string(raw: &str, line_no: usize) -> Result<String, ConfigError> {
    if raw.len() < 2 || !raw.ends_with('"') {
        return Err(ConfigError::parse(
            line_no,
            None,
            "unterminated string literal",
        ));
    }
    let mut chars = raw[1..raw.len() - 1].chars();
    let mut out = String::new();
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => out.push(other),
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }

    if escaped {
        return Err(ConfigError::parse(
            line_no,
            None,
            "dangling escape in string literal",
        ));
    }

    Ok(out)
}

fn parse_array(raw: &str, line_no: usize) -> Result<Vec<TomlValue>, ConfigError> {
    if !raw.ends_with(']') {
        return Err(ConfigError::parse(line_no, None, "unterminated array"));
    }

    let inner = &raw[1..raw.len() - 1];
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    // Split on commas, but only when not inside a quoted string.
    for ch in inner.chars() {
        if in_string {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                current.push(ch);
            }
            ',' => {
                let token = current.trim();
                if !token.is_empty() {
                    values.push(parse_value(token, line_no)?);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if in_string {
        return Err(ConfigError::parse(
            line_no,
            None,
            "unterminated string in array",
        ));
    }

    let token = current.trim();
    if !token.is_empty() {
        values.push(parse_value(token, line_no)?);
    }

    Ok(values)
}

fn ensure_no_unknown_keys(entries: &BTreeMap<String, TomlValue>) -> Result<(), ConfigError> {
    for key in entries.keys() {
        if !is_known_key(key) {
            // Unknown keys are rejected to prevent silent misconfiguration.
            return Err(ConfigError::Parse {
                line: None,
                key: Some(key.clone()),
                message: "unknown configuration key".to_string(),
            });
        }
    }
    Ok(())
}

fn is_known_key(key: &str) -> bool {
    matches_any(
        key,
        &[
            "interval_secs",
            "runtime.interval_secs",
            "high_watermark_percent",
            "thresholds.high_watermark_percent",
            "watermarks.high_watermark_percent",
            "target_watermark_percent",
            "thresholds.target_watermark_percent",
            "watermarks.target_watermark_percent",
            "min_unused_age_days",
            "cleanup.min_unused_age_days",
            "max_delete_per_run_gb",
            "cleanup.max_delete_per_run_gb",
            "dry_run",
            "safety.dry_run",
            "runtime.dry_run",
            "allow_missing_image_labels",
            "safety.allow_missing_image_labels",
            "enabled_backends",
            "backends.enabled_backends",
            "protected_images",
            "allowlists.protected_images",
            "safety.protected_images",
            "protected_volumes",
            "allowlists.protected_volumes",
            "safety.protected_volumes",
            "protected_labels",
            "allowlists.protected_labels",
            "safety.protected_labels",
            "docker_host",
            "docker.host",
            "docker_context",
            "docker.context",
        ],
    )
}

fn matches_any(key: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| key == *candidate)
}

fn take_u64(
    entries: &BTreeMap<String, TomlValue>,
    aliases: &[&str],
    default: u64,
    field_name: &str,
) -> Result<u64, ConfigError> {
    match find_single(entries, aliases, field_name)? {
        Some(TomlValue::Integer(value)) if *value >= 0 => Ok(*value as u64),
        Some(_) => Err(ConfigError::validation(format!(
            "{field_name} must be an integer"
        ))),
        None => Ok(default),
    }
}

fn take_u8(
    entries: &BTreeMap<String, TomlValue>,
    aliases: &[&str],
    default: u8,
    field_name: &str,
) -> Result<u8, ConfigError> {
    let value = take_u64(entries, aliases, default as u64, field_name)?;
    if value > u8::MAX as u64 {
        return Err(ConfigError::validation(format!(
            "{field_name} exceeds u8 range"
        )));
    }
    Ok(value as u8)
}

fn take_bool(
    entries: &BTreeMap<String, TomlValue>,
    aliases: &[&str],
    default: bool,
    field_name: &str,
) -> Result<bool, ConfigError> {
    match find_single(entries, aliases, field_name)? {
        Some(TomlValue::Bool(value)) => Ok(*value),
        Some(_) => Err(ConfigError::validation(format!(
            "{field_name} must be a boolean"
        ))),
        None => Ok(default),
    }
}

fn take_string_array(
    entries: &BTreeMap<String, TomlValue>,
    aliases: &[&str],
    default: Vec<String>,
    field_name: &str,
) -> Result<Vec<String>, ConfigError> {
    match find_single(entries, aliases, field_name)? {
        Some(TomlValue::Array(values)) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    TomlValue::String(item) => out.push(item.clone()),
                    _ => {
                        return Err(ConfigError::validation(format!(
                            "{field_name} must contain only strings"
                        )))
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(ConfigError::validation(format!(
            "{field_name} must be an array"
        ))),
        None => Ok(default),
    }
}

fn take_optional_string(
    entries: &BTreeMap<String, TomlValue>,
    aliases: &[&str],
    default: Option<String>,
    field_name: &str,
) -> Result<Option<String>, ConfigError> {
    match find_single(entries, aliases, field_name)? {
        Some(TomlValue::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ConfigError::validation(format!(
            "{field_name} must be a string"
        ))),
        None => Ok(default),
    }
}

fn find_single<'a>(
    entries: &'a BTreeMap<String, TomlValue>,
    aliases: &[&str],
    field_name: &str,
) -> Result<Option<&'a TomlValue>, ConfigError> {
    let mut found: Option<&TomlValue> = None;

    for alias in aliases {
        if let Some(value) = entries.get(*alias) {
            if found.is_some() {
                return Err(ConfigError::validation(format!(
                    "{field_name} is defined more than once"
                )));
            }
            found = Some(value);
        }
    }

    Ok(found)
}
