use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pal_asset::{vfs, Nls};
use pal_script::read_u32_le;

#[derive(Clone, Debug, Default)]
pub struct EngineStartupConfig {
    pub config_dat: Option<ConfigDat>,
    pub system_dat: Option<SystemDat>,
    pub system_ini: Option<IniFile>,
}

impl EngineStartupConfig {
    pub fn load(root: &Path, nls: Nls) -> anyhow::Result<Self> {
        Ok(Self {
            config_dat: ConfigDat::load_optional(root)?,
            system_dat: SystemDat::load_optional(root)?,
            system_ini: load_system_ini(root, nls)?,
        })
    }

    pub fn window_width(&self, fallback: u32) -> u32 {
        self.config_dat
            .as_ref()
            .map_or(fallback, |config| config.width)
    }

    pub fn window_height(&self, fallback: u32) -> u32 {
        self.config_dat
            .as_ref()
            .map_or(fallback, |config| config.height)
    }

    pub fn window_size(&self, fallback_width: u32, fallback_height: u32) -> (u32, u32) {
        (
            self.window_width(fallback_width),
            self.window_height(fallback_height),
        )
    }

    pub fn logical_size(&self, fallback_width: u32, fallback_height: u32) -> (u32, u32) {
        ini_graphics_size(self.system_ini.as_ref(), fallback_width, fallback_height)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDat {
    pub path: PathBuf,
    pub window_state_0: u32,
    pub window_state_1: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
    pub window_x: u32,
    pub window_y: u32,
    pub field_1c: u32,
    pub field_20: u32,
}

impl ConfigDat {
    pub const FILE_NAME: &'static str = "config.dat";
    pub const SIZE: usize = 0x24;

    pub fn load_optional(root: &Path) -> anyhow::Result<Option<Self>> {
        let path = root.join(Self::FILE_NAME);
        if !vfs::is_file(&path) {
            return Ok(None);
        }
        let bytes = vfs::read_all(&path)
            .map_err(|source| anyhow::anyhow!("failed to read {}: {}", path.display(), source))?;
        if bytes.len() != Self::SIZE {
            return Err(anyhow::anyhow!(
                "invalid {} length: got 0x{:X}, expected 0x{:X}",
                path.display(),
                bytes.len(),
                Self::SIZE
            ));
        }
        Ok(Some(Self::parse(path, &bytes)?))
    }

    pub fn parse(path: PathBuf, bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() != Self::SIZE {
            return Err(anyhow::anyhow!(
                "config.dat must be exactly 0x{:X} bytes, got 0x{:X}",
                Self::SIZE,
                bytes.len()
            ));
        }
        Ok(Self {
            path,
            window_state_0: read_u32_le(bytes, 0x00)?,
            window_state_1: read_u32_le(bytes, 0x04)?,
            width: read_u32_le(bytes, 0x08)?,
            height: read_u32_le(bytes, 0x0C)?,
            flags: read_u32_le(bytes, 0x10)?,
            window_x: read_u32_le(bytes, 0x14)?,
            window_y: read_u32_le(bytes, 0x18)?,
            field_1c: read_u32_le(bytes, 0x1C)?,
            field_20: read_u32_le(bytes, 0x20)?,
        })
    }

    pub fn clamped_window_size(&self, min_width: u32, min_height: u32) -> (u32, u32) {
        (self.width.max(min_width), self.height.max(min_height))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemDat {
    pub path: PathBuf,
    pub marker: [u8; 2],
    pub script_check_value: u32,
    pub fixed_block: Vec<u8>,
    pub raw_tail: Vec<u8>,
}

impl SystemDat {
    pub const FILE_NAME: &'static str = "system.dat";
    pub const MARKER_SIZE: usize = 2;
    pub const FIXED_BLOCK_SIZE: usize = 0x330;

    pub fn load_optional(root: &Path) -> anyhow::Result<Option<Self>> {
        let path = root.join(Self::FILE_NAME);
        if !vfs::is_file(&path) {
            return Ok(None);
        }
        let bytes = vfs::read_all(&path)
            .map_err(|source| anyhow::anyhow!("failed to read {}: {}", path.display(), source))?;
        Ok(Some(Self::parse(path, &bytes)?))
    }

    pub fn parse(path: PathBuf, bytes: &[u8]) -> anyhow::Result<Self> {
        let min_len = Self::MARKER_SIZE + 4;
        if bytes.len() < min_len {
            return Err(anyhow::anyhow!(
                "system.dat is too short: got 0x{:X}, expected at least 0x{:X}",
                bytes.len(),
                min_len
            ));
        }
        let marker = [bytes[0], bytes[1]];
        let fixed_end = (Self::MARKER_SIZE + Self::FIXED_BLOCK_SIZE).min(bytes.len());
        let fixed_block = bytes[Self::MARKER_SIZE..fixed_end].to_vec();
        let script_check_value = read_u32_le(bytes, Self::MARKER_SIZE)?;
        let raw_tail = if fixed_end < bytes.len() {
            bytes[fixed_end..].to_vec()
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            marker,
            script_check_value,
            fixed_block,
            raw_tail,
        })
    }

    pub fn matches_script_check(&self, script_check_value: u32) -> bool {
        self.script_check_value == script_check_value
    }
}

/// A key-value map parsed from a single section of an INI file.
pub type IniSection = BTreeMap<String, IniValue>;

/// INI value: either a bare string or a quoted string literal.
/// All keys and values are decoded from the file's NLS encoding.
#[derive(Clone, Debug, PartialEq)]
pub enum IniValue {
    Str(String),
    Int(i64),
}

impl IniValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            Self::Int(_) => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Str(s) => s.parse::<i64>().ok(),
            Self::Int(n) => Some(*n),
        }
    }
}

/// Separator between the per-language fields of one FHD text record.
pub const TEXT_FIELD_SEPARATOR: char = '\u{203B}';

/// Which language the engine reads out of the text pool.
///
/// The original SoftPAL tables store exactly one NLS-encoded string per
/// record, so they render identically for every value. The later FHD releases
/// replaced the pool with UTF-16LE records that join one field per language
/// with U+203B (`日文※简体※繁体`); the native engine selects a field through
/// its `AdvCommandText0`/`AdvCommandText1`/`AdvCommandText2` variants, which
/// this index mirrors. `Jp` is the default so an unmodified game root keeps
/// the historical behaviour.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextLang {
    #[default]
    Jp,
    Cn,
    Tc,
}

impl TextLang {
    /// Zero-based text-record field this language selects.
    pub fn field_index(self) -> usize {
        match self {
            Self::Jp => 0,
            Self::Cn => 1,
            Self::Tc => 2,
        }
    }

    /// Language named by a multi-language text-record field index, the inverse
    /// of [`Self::field_index`]. Out-of-range values name no field.
    pub fn from_field_index(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Jp),
            1 => Some(Self::Cn),
            2 => Some(Self::Tc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jp => "jp",
            Self::Cn => "cn",
            Self::Tc => "tc",
        }
    }

    /// Per-language archive path list, shipped next to `archive.dat` by the
    /// localized FHD releases. It names the archives carrying the translated
    /// screens, which are searched ahead of the base archives. `None` keeps the
    /// original single-language search path.
    pub fn archive_list(self) -> Option<&'static str> {
        match self {
            Self::Jp => None,
            Self::Cn => Some("archive_cn.dat"),
            Self::Tc => Some("archive_tc.dat"),
        }
    }
}

impl std::str::FromStr for TextLang {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "jp" | "ja" | "jpn" => Ok(Self::Jp),
            "cn" | "zh" | "zh-cn" | "sc" | "chs" => Ok(Self::Cn),
            "tc" | "zh-tw" | "zh-hk" | "cht" => Ok(Self::Tc),
            other => Err(format!(
                "unknown language {other:?} (expected jp, cn or tc)"
            )),
        }
    }
}

/// All sections of an INI file, keyed by lowercased section name.
pub type IniFile = BTreeMap<String, IniSection>;

pub fn ini_graphics_size(
    ini: Option<&IniFile>,
    fallback_width: u32,
    fallback_height: u32,
) -> (u32, u32) {
    let width = ini
        .and_then(|ini| ini.get("graphics"))
        .and_then(|section| section.get("def_cg_width"))
        .and_then(|value| value.as_int())
        .filter(|value| *value > 0)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback_width);
    let height = ini
        .and_then(|ini| ini.get("graphics"))
        .and_then(|section| section.get("def_cg_height"))
        .and_then(|value| value.as_int())
        .filter(|value| *value > 0)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback_height);
    (width.max(1), height.max(1))
}

/// Parse a raw byte buffer as an NLS-encoded INI file.
///
/// Format rules implemented:
/// - `;` at the start of a trimmed line is a comment.
/// - `[section]` starts a new section.
/// - `key = value` sets a key in the current section.
/// - Quoted string values (`"..."`) have quotes stripped.
/// - All text decoded with `nls.decode()`; fails hard if encoding error (no lossy fallback).
/// - Only semantically-string fields are decoded; binary-format files must not use this parser.
pub fn parse_ini_nls(bytes: &[u8], nls: Nls) -> anyhow::Result<IniFile> {
    let text = nls
        .decode(bytes)
        .map_err(|e| anyhow::anyhow!("INI NLS decode failed ({nls:?}): {e}"))?;
    parse_ini_text(&text)
}

/// Parse `SYSTEM.INI`, which is engine configuration rather than game text, so
/// the game's text encoding must not be able to drop it.
///
/// The caller's NLS picks how the game's own scripts and strings are read. The
/// FHD Chinese releases keep `SYSTEM.INI` inside `data.pac` in the engine's
/// Shift-JIS: importing one as UTF-8 made the strict decode fail, the release's
/// logical size and text size silently fell back to the PAL defaults
/// (1280x720, 28px), and the title screen came out cropped to the top-left of
/// its layout. Read the caller's encoding first and keep the engine's own as
/// the fallback, the way the native engine reads this file.
pub fn parse_system_ini(bytes: &[u8], nls: Nls) -> anyhow::Result<IniFile> {
    match parse_ini_nls(bytes, nls) {
        Ok(ini) => {
            log::info!("SYSTEM.INI decoded as {nls:?}");
            Ok(ini)
        }
        Err(requested) => match parse_ini_nls(bytes, Nls::ShiftJis) {
            Ok(ini) => {
                log::warn!(
                    "SYSTEM.INI does not decode as {nls:?} ({requested}); \
                     read as the engine's Shift-Jis"
                );
                Ok(ini)
            }
            Err(engine) => Err(anyhow::anyhow!(
                "SYSTEM.INI decode failed as {nls:?} ({requested}) and as Shift-Jis ({engine})"
            )),
        },
    }
}

/// Load and parse an INI file from disk with NLS decoding.
pub fn load_ini_nls(path: &Path, nls: Nls) -> anyhow::Result<IniFile> {
    let bytes = vfs::read_all(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    parse_system_ini(&bytes, nls).map_err(|e| anyhow::anyhow!("{} (file: {})", e, path.display()))
}

fn parse_ini_text(text: &str) -> anyhow::Result<IniFile> {
    let mut result: IniFile = BTreeMap::new();
    let mut current_section = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }
        if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current_section = inner.trim().to_ascii_lowercase();
            result.entry(current_section.clone()).or_default();
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_ascii_lowercase();
            let raw_value = trimmed[eq_pos + 1..].trim();
            if key.is_empty() {
                continue;
            }
            let value = if let Some(inner) = raw_value
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
            {
                IniValue::Str(inner.to_owned())
            } else if let Ok(n) = raw_value.parse::<i64>() {
                IniValue::Int(n)
            } else {
                IniValue::Str(raw_value.to_owned())
            };
            result
                .entry(current_section.clone())
                .or_default()
                .insert(key, value);
        }
    }
    Ok(result)
}

/// Load SYSTEM.INI from the game root with NLS decoding.
/// Returns `None` if the file does not exist.
pub fn load_system_ini(root: &Path, nls: Nls) -> anyhow::Result<Option<IniFile>> {
    let path = root.join("SYSTEM.INI");
    if !vfs::is_file(&path) {
        return Ok(None);
    }
    Ok(Some(load_ini_nls(&path, nls)?))
}
