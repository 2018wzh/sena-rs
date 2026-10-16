use pal_asset::{Nls, ResourceManager};
use pal_vm::config::ini_graphics_size;
use pal_vm::{parse_ini_nls, parse_system_ini, EngineStartupConfig, FrameScene, IniValue};
use std::path::PathBuf;

#[test]
fn parse_simple_ini_ascii() {
    let ini = b"[section]\nkey = value\nnum = 42\n";
    let result = parse_ini_nls(ini, Nls::Utf8).expect("should parse ASCII INI");
    let section = result.get("section").expect("section must exist");
    assert_eq!(section.get("key"), Some(&IniValue::Str("value".to_owned())));
    assert_eq!(section.get("num"), Some(&IniValue::Int(42)));
}

#[test]
fn parse_ini_quoted_string() {
    let ini = b"[s]\nFILE = \"default_font\"\n";
    let result = parse_ini_nls(ini, Nls::Utf8).expect("parse");
    let section = result.get("s").unwrap();
    assert_eq!(
        section.get("file"),
        Some(&IniValue::Str("default_font".to_owned()))
    );
}

#[test]
fn parse_ini_comments_and_blank_lines_ignored() {
    let ini = b"; this is a comment\n\n[a]\n; another comment\nk = 1\n";
    let result = parse_ini_nls(ini, Nls::Utf8).expect("parse");
    let a = result.get("a").unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a.get("k"), Some(&IniValue::Int(1)));
}

#[test]
fn parse_ini_section_names_are_lowercased() {
    let ini = b"[Graphics]\nDEF_CG_WIDTH = 1280\n";
    let result = parse_ini_nls(ini, Nls::Utf8).expect("parse");
    assert!(
        result.contains_key("graphics"),
        "section key should be lowercased"
    );
    let g = result.get("graphics").unwrap();
    assert_eq!(g.get("def_cg_width"), Some(&IniValue::Int(1280)));
}

#[test]
fn parse_ini_negative_int() {
    let ini = b"[x]\nSAMPLE_TEXT_FONT_FIXED = -1\n";
    let result = parse_ini_nls(ini, Nls::Utf8).expect("parse");
    let v = result
        .get("x")
        .unwrap()
        .get("sample_text_font_fixed")
        .unwrap();
    assert_eq!(v.as_int(), Some(-1));
}

#[test]
fn ini_value_as_int_parses_str_fallback() {
    let v = IniValue::Str("99".to_owned());
    assert_eq!(v.as_int(), Some(99));
}

#[test]
fn ini_value_int_as_str_returns_none() {
    let v = IniValue::Int(7);
    assert_eq!(v.as_str(), None);
}

#[test]
fn parse_sjis_ini_does_not_use_lossy() {
    // Valid SJIS bytes for the string "[section]\nk = 1\n"
    // (this is pure ASCII so valid in SJIS as well)
    let ini = b"[section]\nk = 1\n";
    let result = parse_ini_nls(ini, Nls::ShiftJis).expect("SJIS parse of ASCII INI");
    assert_eq!(
        result.get("section").unwrap().get("k"),
        Some(&IniValue::Int(1))
    );
}

#[test]
fn system_ini_falls_back_to_the_engine_encoding() {
    // A Shift-JIS comment (テ, 0x83 0x65, which is not valid UTF-8) ahead of the
    // keys the engine reads: the launcher's text encoding must not be able to
    // drop the release's own size.
    let mut bytes = vec![b';', 0x83, 0x65];
    bytes.extend_from_slice(b" config\r\n[graphics]\r\ndef_cg_width = 1920\r\ndef_cg_height = 1080\r\n");
    let result = parse_system_ini(&bytes, Nls::Utf8).expect("Shift-JIS fallback");
    assert_eq!(ini_graphics_size(Some(&result), 0, 0), (1920, 1080));
}

#[test]
fn chinese_fhd_archived_system_ini_survives_a_utf8_caller() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("testcase-cn/想要传达给你的爱恋");
    if !root.join("data.pac").is_file() {
        eprintln!("skipping: the Chinese FHD fixture is not present");
        return;
    }
    let mut manager = ResourceManager::bootstrap_with_language(&root, Nls::Utf8, None)
        .expect("bootstrap the Chinese release");
    let asset = manager.open("SYSTEM.INI").expect("archived SYSTEM.INI");
    let ini = parse_system_ini(&asset.bytes, Nls::Utf8).expect("parse");
    assert_eq!(
        ini_graphics_size(Some(&ini), 0, 0),
        (1920, 1080),
        "the FHD release's logical size must survive any caller encoding"
    );
}

#[test]
fn startup_config_reads_system_ini_logical_size_before_vm_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("testcase");
    let config = EngineStartupConfig::load(&root, Nls::ShiftJis).expect("startup config");
    assert!(config.system_ini.is_some(), "SYSTEM.INI must be loaded");
    assert_eq!(
        config.logical_size(
            FrameScene::PAL_DEFAULT_WIDTH,
            FrameScene::PAL_DEFAULT_HEIGHT
        ),
        (1280, 720)
    );
}
