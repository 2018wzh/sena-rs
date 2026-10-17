use super::*;

#[test]
fn query_returns_the_same_complete_table() {
    unsafe {
        let mut first = std::ptr::null();
        let mut second = std::ptr::null();
        assert_eq!(QUERY(ABI_VERSION, &mut first), OK);
        assert_eq!(QUERY(ABI_VERSION, &mut second), OK);
        assert_eq!(first, second);
        assert_eq!((*first).validate(), Ok(()));
        assert_eq!(QUERY(ABI_VERSION + 1, &mut second), UNSUPPORTED);
        assert!(second.is_null());
    }
}

#[test]
fn startup_schema_rejects_unknown_and_out_of_range_values() {
    unsafe {
        assert_eq!(validate_config(&config::SCHEMA.0, &[]), Ok(()));
        let mut entry = ConfigEntry { id: text("raster_width"), value: ConfigValue {
            kind: VALUE_INTEGER, reserved: 0, integer: 3840, number: 0.0, text: Bytes::EMPTY,
        }};
        assert_eq!(validate_config(&config::SCHEMA.0, &[entry]), Ok(()));
        entry.value.integer = 3841;
        assert!(validate_config(&config::SCHEMA.0, &[entry]).is_err());
        entry.value.integer = 1280;
        entry.id = text("window_width");
        assert!(validate_config(&config::SCHEMA.0, &[entry]).is_err());
    }
}

#[test]
fn released_cpu_lease_needs_no_session_or_device() {
    let pixels = Arc::new(vec![17, 31, 47, 255]);
    let count = Arc::new(AtomicUsize::new(1));
    let lease = Box::new(Lease { pixels: Arc::clone(&pixels), count: Arc::clone(&count) });
    let bytes = Bytes::new(&lease.pixels);
    let pointer = Box::into_raw(lease).cast();
    drop(pixels);
    unsafe {
        assert_eq!(bytes.as_slice(4).unwrap(), &[17, 31, 47, 255]);
        release(pointer);
    }
    assert_eq!(count.load(Ordering::Acquire), 0);
}

#[test]
fn raster_configuration_is_independent_of_logical_extent() {
    let settings = config::Settings { root: "unused-test-path".into(), width: 1920, height: 1080, language: pal_vm::TextLang::Jp };
    let info = config::surface(&settings, 1280, 720);
    assert_eq!(info.validate(), Ok(()));
    assert_eq!(info.logical, Extent { width: 1280, height: 720 });
    assert_eq!(info.raster, Extent { width: 1920, height: 1080 });
    assert_eq!(info.stride, 7680);
}

#[test]
fn probe_recognizes_only_koikake_executable_or_steam_identity() {
    let root = std::env::temp_dir().join(format!("sena-probe-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    assert!(!config::matches_game(&root));
    std::fs::write(root.join("Game.exe"), []).unwrap();
    assert!(!config::matches_game(&root));
    std::fs::write(root.join("steam_appid.txt"), "2242711").unwrap();
    assert!(!config::matches_game(&root));
    std::fs::write(root.join("steam_appid.txt"), "2242710\n").unwrap();
    assert!(config::matches_game(&root));
    std::fs::write(root.join("steam_appid.txt"), [b'2'; 33]).unwrap();
    assert!(!config::matches_game(&root));
    std::fs::remove_file(root.join("Game.exe")).unwrap();
    std::fs::write(root.join("steam_appid.txt"), "2242710").unwrap();
    assert!(!config::matches_game(&root));
    std::fs::create_dir(root.join("koikake.exe")).unwrap();
    assert!(!config::matches_game(&root));
    std::fs::remove_dir(root.join("koikake.exe")).unwrap();
    std::fs::write(root.join("KOIKAKE.EXE"), []).unwrap();
    assert!(config::matches_game(&root));
    std::fs::remove_dir_all(root).unwrap();
}
