use wgc_download::torrent::parse_torrent;

const BUILD_TORRENT: &[u8] = include_bytes!("fixtures/build.torrent");

#[test]
fn parses_real_build_torrent_fixture() {
    let meta = parse_torrent(BUILD_TORRENT).expect("build.torrent should parse");

    assert!(meta.name.starts_with("wows_15.8.0.0.13090112"), "unexpected name: {}", meta.name);
    assert_eq!(meta.piece_length, 2_097_152);

    // Real torrent has 6 real WG .dspkg entries plus 5 BitTorrent
    // piece-alignment padding entries (BEP47 `attr=p`), which carry no sha1
    // and are not real download content; the parser drops the padding.
    assert_eq!(meta.files.len(), 6);
    assert_eq!(meta.total_length, 92_546_639_270);
    assert!(meta.total_length > 0);

    for file in &meta.files {
        let sha1 = file.sha1.as_deref().expect("every real WG file entry should carry a sha1");
        assert_eq!(sha1.len(), 40, "sha1 should be a 40-char hex string, got {sha1:?}");
    }

    let client = meta
        .files
        .iter()
        .find(|f| f.relative_path() == "wows.na_15.8.0.0.13090112_client.dspkg")
        .expect("client dspkg present");
    assert_eq!(client.length, 67_050_140_149);
    assert_eq!(client.sha1.as_deref(), Some("6dc427fc0d01942223c0ccabf72d6ae7f4ec5efe"));

    let hotfix = meta
        .files
        .iter()
        .find(|f| f.relative_path() == "wows.na_15.8.0.0.13090112_hotfix.dspkg")
        .expect("hotfix dspkg present");
    assert_eq!(hotfix.length, 181);
    assert_eq!(hotfix.sha1.as_deref(), Some("4d959983bc384ca25999bd9f96b646ee7659a6c4"));

    assert!(!meta.unpacked_sizes.is_empty());
    let client_unpacked = meta
        .unpacked_sizes
        .get("wows.na_15.8.0.0.13090112_client.dspkg")
        .copied()
        .expect("client.dspkg key present in unpacked_patches_size");
    assert_eq!(client_unpacked, 67_794_517_173);
    assert!(client_unpacked > 1_000_000_000, "expected a multi-GB unpacked client size");
}

#[test]
fn synthetic_torrent_round_trips_fields() {
    let bytes = build_synthetic_torrent();
    let meta = parse_torrent(&bytes).expect("synthetic torrent should parse");

    assert_eq!(meta.name, "synth-build");
    assert_eq!(meta.piece_length, 16_384);
    assert_eq!(meta.total_length, 111_222_333 + 444_555);

    assert_eq!(meta.files.len(), 2);

    let client = meta.files.iter().find(|f| f.relative_path() == "build/client.dspkg").expect("client entry present");
    assert_eq!(client.length, 111_222_333);
    assert_eq!(client.sha1.as_deref(), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));

    let locale = meta.files.iter().find(|f| f.relative_path() == "build/locale.dspkg").expect("locale entry present");
    assert_eq!(locale.length, 444_555);
    assert_eq!(locale.sha1.as_deref(), Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));

    assert_eq!(meta.unpacked_sizes.get("synth.dspkg").copied(), Some(123_456_789));
}

#[test]
fn missing_comment_yields_empty_unpacked_sizes() {
    let bytes = build_synthetic_torrent_without_comment();
    let meta = parse_torrent(&bytes).expect("torrent without a comment should still parse");
    assert!(meta.unpacked_sizes.is_empty());
}

// --- Hand-built bencode helpers (no serde_bencode encoder involved: these
// concatenate raw bencode wire bytes directly, independent of the crate's
// own decoder, so the tests exercise the decoder against known-correct
// bytes rather than round-tripping through the same library). ---

fn benc_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("{}:", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out
}

fn benc_str(s: &str) -> Vec<u8> {
    benc_bytes(s.as_bytes())
}

fn benc_int(n: i64) -> Vec<u8> {
    format!("i{n}e").into_bytes()
}

fn benc_list(items: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = vec![b'l'];
    for item in items {
        out.extend(item);
    }
    out.push(b'e');
    out
}

fn benc_dict(pairs: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut out = vec![b'd'];
    for (key, value) in pairs {
        out.extend(benc_str(key));
        out.extend(value);
    }
    out.push(b'e');
    out
}

fn benc_file(path: &[&str], length: i64, sha1: [u8; 20]) -> Vec<u8> {
    benc_dict(vec![
        ("length", benc_int(length)),
        ("path", benc_list(path.iter().map(|s| benc_str(s)).collect())),
        ("sha1", benc_bytes(&sha1)),
    ])
}

fn build_synthetic_torrent() -> Vec<u8> {
    let comment = r#"{"unpacked_patches_size":{"synth.dspkg":123456789}}"#;
    let files = benc_list(vec![
        benc_file(&["build", "client.dspkg"], 111_222_333, [0xAA; 20]),
        benc_file(&["build", "locale.dspkg"], 444_555, [0xBB; 20]),
    ]);
    let info = benc_dict(vec![
        ("files", files),
        ("name", benc_str("synth-build")),
        ("piece length", benc_int(16_384)),
        ("pieces", benc_bytes(&[0xCC; 20])),
    ]);
    benc_dict(vec![("comment", benc_str(comment)), ("info", info)])
}

fn build_synthetic_torrent_without_comment() -> Vec<u8> {
    let files = benc_list(vec![benc_file(&["build", "client.dspkg"], 111_222_333, [0xAA; 20])]);
    let info = benc_dict(vec![
        ("files", files),
        ("name", benc_str("synth-build")),
        ("piece length", benc_int(16_384)),
        ("pieces", benc_bytes(&[0xCC; 20])),
    ]);
    benc_dict(vec![("info", info)])
}
