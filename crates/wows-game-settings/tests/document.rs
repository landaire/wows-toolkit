mod common;

use wows_game_settings::AttrName;
use wows_game_settings::PrefsDocument;
use wows_game_settings::PrefsEdit;
use wows_game_settings::PrefsEditError;
use wows_game_settings::PrefsPath;

use crate::common::fixture;

const FIXTURES: [&str; 4] = ["preferences.xml", "user_preferences.xml", "engine_config.xml", "game_info.xml"];

fn set_value(path: &'static str, value: &str) -> PrefsEdit {
    PrefsEdit::SetValue { path: PrefsPath::new(path), value: value.to_owned() }
}

#[test]
fn parse_and_render_is_byte_identical() {
    for name in FIXTURES {
        let source = fixture(name);
        let document = PrefsDocument::parse(source.clone()).unwrap();

        assert_eq!(document.text(), source, "{name}");
    }
}

#[test]
fn writing_the_current_value_changes_nothing() {
    let source = fixture("preferences.xml");
    let mut document = PrefsDocument::parse(source.clone()).unwrap();

    document.set(&set_value("scriptsPreferences/renderer/maxFrameRate", "0")).unwrap();
    document.set(&set_value("scriptsPreferences/net_credentials/demo", "")).unwrap();

    assert_eq!(document.text(), source);
}

#[test]
fn value_edit_keeps_padding_and_line_endings() {
    let source = fixture("preferences.xml");
    let mut document = PrefsDocument::parse(source.clone()).unwrap();

    document.set(&set_value("scriptsPreferences/renderer/maxFrameRate", "144")).unwrap();

    let expected =
        source.replacen("<maxFrameRate>\t0\t</maxFrameRate>\r\n", "<maxFrameRate>\t144\t</maxFrameRate>\r\n", 1);
    assert_eq!(document.text(), expected);
    assert_eq!(document.value(&PrefsPath::new("scriptsPreferences/renderer/maxFrameRate")), Some("144"));
}

#[test]
fn attribute_edit_keeps_bom_and_other_attributes() {
    let source = fixture("user_preferences.xml");
    let mut document = PrefsDocument::parse(source.clone()).unwrap();
    let device = PrefsPath::new("local/device");
    let window_mode = AttrName::new("windowMode");

    document
        .set(&PrefsEdit::SetAttribute {
            path: device.clone(),
            name: window_mode.clone(),
            value: "FULLSCREEN".to_owned(),
        })
        .unwrap();

    let expected = source.replacen(
        r#"<device version="2" windowMode="BORDERLESS""#,
        r#"<device version="2" windowMode="FULLSCREEN""#,
        1,
    );
    assert!(document.text().starts_with('\u{FEFF}'));
    assert_eq!(document.text(), expected);
    assert_eq!(document.attribute(&device, &window_mode), Some("FULLSCREEN"));
}

#[test]
fn value_edit_fills_a_self_closing_element_with_the_file_padding() {
    let source = fixture("preferences.xml");
    let mut document = PrefsDocument::parse(source.clone()).unwrap();

    document.set(&set_value("scriptsPreferences/net_credentials/demo", "1")).unwrap();

    let expected = source.replacen("<demo />", "<demo >\t1\t</demo>", 1);
    assert_eq!(document.text(), expected);
}

#[test]
fn tag_names_with_digits_and_at_signs_resolve() {
    let document = PrefsDocument::parse(fixture("preferences.xml")).unwrap();
    let chat_box = PrefsPath::new("scriptsPreferences/ui/chatBoxPositionX");
    let first = document.child_names(&chat_box).unwrap().next().unwrap().to_owned();
    let path = PrefsPath::parse(&format!("scriptsPreferences/ui/chatBoxPositionX/{first}")).unwrap();

    assert!(first.contains('@'));
    assert!(first.starts_with(|character: char| character.is_ascii_digit()));
    assert!(document.value(&path).is_some());
}

#[test]
fn rejected_edits() {
    let mut document = PrefsDocument::parse(fixture("user_preferences.xml")).unwrap();
    let before = document.clone();
    let edits = [
        set_value("local/missing", "1"),
        set_value("local/device", "1"),
        set_value("local/ui/skipVideo", "a<b"),
        set_value("local/ui/skipVideo", " true"),
        PrefsEdit::SetAttribute {
            path: PrefsPath::new("local/device"),
            name: AttrName::new("missing"),
            value: "1".to_owned(),
        },
        PrefsEdit::SetAttribute {
            path: PrefsPath::new("local/device"),
            name: AttrName::new("windowMode"),
            value: "a\"b".to_owned(),
        },
    ];

    let errors: Vec<PrefsEditError> = edits.iter().map(|edit| document.set(edit).unwrap_err()).collect();

    assert_eq!(document, before);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn value_edit_refuses_to_drop_markup() {
    let source = "<root>\n  <a>1<!-- note --></a>\n  <b><![CDATA[2]]></b>\n  <c x=\"p>q\"/>\n</root>\n".to_owned();
    let mut document = PrefsDocument::parse(source.clone()).unwrap();
    let c = PrefsPath::new("c");
    let x = AttrName::new("x");

    let errors = [set_value("a", "3"), set_value("b", "3")].map(|edit| document.set(&edit).unwrap_err());
    document.set(&PrefsEdit::SetAttribute { path: c.clone(), name: x.clone(), value: "r>s".to_owned() }).unwrap();

    assert_eq!(document.text(), source.replacen("p>q", "r>s", 1));
    insta::assert_debug_snapshot!(errors);
}
