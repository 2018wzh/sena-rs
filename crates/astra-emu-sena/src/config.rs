use astra_emu_abi::*;
use pal_vm::TextLang;
use std::path::{Path, PathBuf};

pub const fn text(value: &'static str) -> Bytes {
    Bytes { data: value.as_ptr(), len: value.len() }
}
struct Choices([Bytes; 3]);
// All pointers refer to immutable library-lifetime strings.
unsafe impl Sync for Choices {}
static LANGUAGES: Choices = Choices([text("jp"), text("cn"), text("tc")]);
pub struct Schema(pub [ConfigField; 3]);
unsafe impl Sync for Schema {}
const fn dimension(id: &'static str, label: &'static str, default: i64) -> ConfigField {
    ConfigField {
        id: text(id), label: text(label), group: text("Renderer"),
        default_value: ConfigValue { kind: VALUE_INTEGER, reserved: 0, integer: default, number: 0.0, text: Bytes::EMPTY },
        integer_min: 320, integer_max: 3840, number_min: 0.0, number_max: 0.0,
        max_bytes: 0, reserved: 0, choices: std::ptr::null(), choice_count: 0,
    }
}
pub static SCHEMA: Schema = Schema([
    dimension("raster_width", "Raster width", 1280),
    dimension("raster_height", "Raster height", 720),
    ConfigField {
        id: text("text_language"), label: text("Native text language"), group: text("Game"),
        default_value: ConfigValue { kind: VALUE_ENUM, reserved: 0, integer: 0, number: 0.0, text: text("jp") },
        integer_min: 0, integer_max: 0, number_min: 0.0, number_max: 0.0,
        max_bytes: 0, reserved: 0, choices: LANGUAGES.0.as_ptr(), choice_count: 3,
    },
]);
#[derive(Clone)]
pub struct Settings {
    pub root: PathBuf,
    pub width: u32,
    pub height: u32,
    pub language: TextLang,
}
impl Settings {
    pub unsafe fn read(request: &OpenRequest) -> Result<Self, Status> {
        let values = borrowed_array(request.configuration, request.configuration_count, MAX_CONFIG_FIELDS)?;
        validate_config(&SCHEMA.0, values).map_err(|_| INVALID_ARGUMENT)?;
        let mut result = Self { root: game_root(request.game_path.text(32768)?), width: 1280, height: 720, language: TextLang::Jp };
        for entry in values {
            match entry.id.text(128)? {
                "raster_width" => result.width = entry.value.integer as u32,
                "raster_height" => result.height = entry.value.integer as u32,
                "text_language" => result.language = match entry.value.text.text(128)? {
                    "jp" => TextLang::Jp, "cn" => TextLang::Cn, "tc" => TextLang::Tc,
                    _ => return Err(INVALID_ARGUMENT),
                },
                _ => return Err(INVALID_ARGUMENT),
            }
        }
        Ok(result)
    }
}
pub fn game_root(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_file() { path.parent().unwrap_or(path).to_path_buf() } else { path.to_path_buf() }
}
/// Recognize Koikake's original executable or its exact Steam release identity.
pub fn matches_game(root: &Path) -> bool {
    use std::io::Read;
    let Ok(entries) = std::fs::read_dir(root) else { return false };
    let mut game_exe = false;
    let mut steam_appid = None;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) { continue; }
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if filename.eq_ignore_ascii_case("koikake.exe") { return true; }
        if filename.eq_ignore_ascii_case("game.exe") { game_exe = true; }
        if filename.eq_ignore_ascii_case("steam_appid.txt") { steam_appid = Some(entry.path()); }
    }
    if !game_exe { return false; }
    let Some(path) = steam_appid else { return false };
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let Ok(metadata) = file.metadata() else { return false };
    if !(1..=32).contains(&metadata.len()) { return false; }
    let mut bytes = [0u8; 32];
    let bytes = &mut bytes[..metadata.len() as usize];
    file.read_exact(bytes).is_ok() && bytes.trim_ascii() == b"2242710"
}
pub fn surface(settings: &Settings, width: u32, height: u32) -> SurfaceInfo {
    SurfaceInfo {
        kind: SURFACE_CPU, pixel_format: PIXEL_RGBA8, color: COLOR_SRGB,
        alpha: ALPHA_OPAQUE, rows: ROWS_TOP_DOWN, backend: 0,
        logical: Extent { width, height },
        raster: Extent { width: settings.width, height: settings.height },
        stride: u64::from(settings.width) * 4,
    }
}
