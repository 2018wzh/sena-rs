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
fn descriptor_declares_names_and_render_extent() {
    let diagnostics = Diagnostics { header: Header::of::<Diagnostics>(), context: std::ptr::null_mut(),
        emit: Some(emit), flush: Some(flush) };
    unsafe extern "C" fn emit(_: *mut c_void, _: *const LogRecord) {}
    unsafe extern "C" fn flush(_: *mut c_void) {}
    unsafe {
        assert_eq!(initialize(&diagnostics, std::ptr::null_mut()), OK);
        let mut descriptor = std::mem::zeroed::<Descriptor>();
        assert_eq!(describe(&mut descriptor, std::ptr::null_mut()), OK);
        assert_eq!(descriptor.validate(), Ok(()));
        assert_eq!(descriptor.display_name.text(64), Ok("Sena"));
        assert_eq!(descriptor.family_name.text(64), Ok("SoftPAL"));
        assert_eq!(descriptor.capabilities, CAP_CPU_SURFACE | CAP_RENDER_EXTENT);
        assert_eq!(descriptor.render, config::RENDER);
        let schema = std::slice::from_raw_parts(descriptor.configuration, descriptor.configuration_count);
        assert_eq!(validate_config(schema, &[]), Ok(()));
    }
}

#[test]
fn render_extent_requests_stay_within_declared_range() {
    let capabilities = CAP_CPU_SURFACE | CAP_RENDER_EXTENT;
    assert_eq!(config::RENDER.validate(capabilities), Ok(()));
    assert_eq!(config::RENDER.validate_request(capabilities, Extent::default()), Ok(()));
    assert_eq!(config::RENDER.validate_request(capabilities, Extent { width: 3840, height: 320 }), Ok(()));
    assert_eq!(config::RENDER.validate_request(capabilities, Extent { width: 3841, height: 720 }), Err(INVALID_ARGUMENT));
    assert_eq!(config::RENDER.validate_request(capabilities, Extent { width: 1280, height: 319 }), Err(INVALID_ARGUMENT));
}

#[test]
fn startup_schema_rejects_unknown_and_invalid_values() {
    unsafe {
        assert_eq!(validate_config(&config::SCHEMA.0, &[]), Ok(()));
        let mut entry = ConfigEntry { id: text("text_language"), value: ConfigValue {
            kind: VALUE_ENUM, reserved: 0, integer: 0, number: 0.0, text: text("tc"),
        }};
        assert_eq!(validate_config(&config::SCHEMA.0, &[entry]), Ok(()));
        entry.value.text = text("en");
        assert!(validate_config(&config::SCHEMA.0, &[entry]).is_err());
        entry.value.text = text("jp");
        entry.id = text("raster_width");
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
