use std::collections::HashMap;
use std::collections::HashSet;

/// A single key-binding entry parsed from `commands.scheme.xml`.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ReplayCommand {
    /// Human-readable name derived from the XML tag (e.g. "Free Camera").
    pub label: String,
    /// Context the command belongs to (e.g. "replay", "dead", "freeCam").
    pub context: String,
    /// Primary key binding (e.g. "C", "Shift+F5").
    pub key1: String,
    /// Secondary key binding, if any.
    pub key2: Option<String>,
}

/// A group of related commands for display.
#[derive(Clone, Debug)]
pub struct CommandGroup {
    pub title: &'static str,
    pub commands: Vec<ReplayCommand>,
}

/// Commands from contexts we only partially include (cherry-picked by tag name).
const EXTRA_COMMAND_ALLOWLIST: &[&str] =
    &["CMD_ART_CAMERA", "CMD_OBSERVE_CAMERA", "CMD_SWITCH_CAMERA", "CMD_FREE_CURSOR"];

/// Commands that don't function during replay playback.
const EXCLUDED_COMMANDS: &[&str] = &[
    "CMD_DIVISION_INVITATION_ACCEPT",
    "CMD_DIVISION_INVITATION_DECLINE",
    "CMD_ENABLE_BATTLE_CHAT",
    "CMD_QUICK_COMMANDS_WINDOW",
    "CMD_VOICE_CHAT_TALK",
];

/// Returns true if this command tag should be excluded from the controls list.
fn should_exclude_command(tag_name: &str) -> bool {
    tag_name.contains("REPLAY_UTILS") || EXCLUDED_COMMANDS.contains(&tag_name)
}

/// Parse `system/data/commands.scheme.xml` from raw bytes into grouped commands
/// relevant for replay/spectator usage.
pub(crate) fn parse_commands_scheme(data: &[u8]) -> Vec<CommandGroup> {
    let text = String::from_utf8_lossy(data);
    let doc = match roxmltree::Document::parse(&text) {
        Ok(doc) => doc,
        Err(_) => return Vec::new(),
    };

    // Contexts fully included (all commands)
    let context_groups: &[(&str, &str)] = &[
        ("replay", "Replay Controls"),
        ("dead", "Spectator / Dead"),
        ("freeCam", "Free Camera"),
        ("battle", "Battle / HUD"),
        ("default", "General"),
    ];

    // Extra contexts where only allowlisted commands are included,
    // merged into the "Battle / HUD" group.
    let extra_contexts: HashSet<&str> = ["alive", "cursor", "ship", "sound"].into_iter().collect();

    let mut group_map: HashMap<&str, Vec<ReplayCommand>> = HashMap::new();
    for &(ctx, _) in context_groups {
        group_map.insert(ctx, Vec::new());
    }

    // Find the <commands> element and iterate its children
    let commands_elem = doc.descendants().find(|n| n.has_tag_name("commands"));
    let parent = commands_elem.unwrap_or(doc.root());

    for node in parent.children().filter(|n| n.is_element()) {
        let tag_name = node.tag_name().name();
        let Some(context) = node.attribute("context") else {
            continue;
        };

        if should_exclude_command(tag_name) {
            continue;
        }

        // Determine which group this command belongs to
        let target_group = if group_map.contains_key(context) {
            context
        } else if extra_contexts.contains(context) && EXTRA_COMMAND_ALLOWLIST.contains(&tag_name) {
            "battle" // merge into Battle / HUD
        } else {
            continue;
        };

        // Extract KEY and MODS from VALUE1 and VALUE2 children
        let (key1, key2) = parse_value_children(&node);
        if key1.is_empty() && key2.is_empty() {
            continue;
        }

        let label = humanize_command_name(tag_name);
        let primary = if key1.is_empty() { key2.clone() } else { key1 };
        let secondary = if primary == key2 || key2.is_empty() { None } else { Some(key2) };

        group_map.get_mut(target_group).unwrap().push(ReplayCommand {
            label,
            context: context.to_string(),
            key1: primary,
            key2: secondary,
        });
    }

    context_groups
        .iter()
        .filter_map(|&(ctx, title)| {
            let cmds = group_map.remove(ctx)?;
            if cmds.is_empty() {
                return None;
            }
            Some(CommandGroup { title, commands: cmds })
        })
        .collect()
}

/// Extract formatted keybindings from VALUE1/VALUE2 child elements.
fn parse_value_children(node: &roxmltree::Node<'_, '_>) -> (String, String) {
    let mut key1 = String::new();
    let mut key2 = String::new();

    for child in node.children().filter(|n| n.is_element()) {
        let tag = child.tag_name().name();
        let key_text = child.children().find(|n| n.has_tag_name("KEY")).and_then(|n| n.text()).unwrap_or("KEY_NULL");
        let mods_text = child.children().find(|n| n.has_tag_name("MODS")).and_then(|n| n.text()).unwrap_or("KEY_NULL");

        let formatted = format_keybinding(key_text, mods_text);
        match tag {
            "VALUE1" => key1 = formatted,
            "VALUE2" => key2 = formatted,
            _ => {}
        }
    }

    (key1, key2)
}

/// Convert a `KEY_X` string and `MODS` string to a human-readable keybinding.
fn format_keybinding(key: &str, mods: &str) -> String {
    if key == "KEY_NULL" || key.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();

    // Parse modifier keys
    for m in mods.split_whitespace() {
        match m {
            "KEY_LCONTROL" | "KEY_RCONTROL" => parts.push("Ctrl"),
            "KEY_LSHIFT" | "KEY_RSHIFT" => parts.push("Shift"),
            "KEY_LALT" | "KEY_RALT" => parts.push("Alt"),
            "KEY_NULL" | "" => {}
            _ => {}
        }
    }

    // Convert key name
    let key_display = match key {
        "KEY_LEFTMOUSE" => "LMB",
        "KEY_RIGHTMOUSE" => "RMB",
        "KEY_MIDDLEMOUSE" => "MMB",
        "KEY_SPACE" => "Space",
        "KEY_RETURN" => "Enter",
        "KEY_NUMPADENTER" => "Numpad Enter",
        "KEY_ESCAPE" => "Esc",
        "KEY_TAB" => "Tab",
        "KEY_LSHIFT" => "LShift",
        "KEY_LCONTROL" => "LCtrl",
        "KEY_LALT" => "LAlt",
        "KEY_BACKSPACE" => "Backspace",
        "KEY_DELETE" => "Del",
        "KEY_INSERT" => "Ins",
        "KEY_HOME" => "Home",
        "KEY_END" => "End",
        "KEY_PGUP" => "PgUp",
        "KEY_PGDN" => "PgDn",
        "KEY_UPARROW" => "Up",
        "KEY_DOWNARROW" => "Down",
        "KEY_LEFTARROW" => "Left",
        "KEY_RIGHTARROW" => "Right",
        "KEY_ADD" => "Num +",
        "KEY_NUMPADMINUS" => "Num -",
        "KEY_NUMPADPERIOD" => "Num .",
        "KEY_PERIOD" => ".",
        "KEY_COMMA" => ",",
        "KEY_MINUS" => "-",
        "KEY_EQUALS" => "=",
        "KEY_GRAVE" => "`",
        "KEY_LBRACKET" => "[",
        "KEY_RBRACKET" => "]",
        other => {
            // KEY_F1 -> F1, KEY_A -> A, KEY_1 -> 1, KEY_NUMPAD5 -> Num5
            let stripped = other.strip_prefix("KEY_").unwrap_or(other);
            if let Some(n) = stripped.strip_prefix("NUMPAD") {
                return format!(
                    "{}Num {}",
                    if parts.is_empty() { String::new() } else { format!("{} + ", parts.join(" + ")) },
                    n
                );
            }
            // Leak is fine for static-like strings in a bounded set
            return format!(
                "{}{}",
                if parts.is_empty() { String::new() } else { format!("{} + ", parts.join(" + ")) },
                stripped
            );
        }
    };

    if parts.is_empty() { key_display.to_string() } else { format!("{} + {}", parts.join(" + "), key_display) }
}

/// Convert a command tag name like `CMD_REPLAY_FREE_CAMERA` to `"Free Camera"`.
fn humanize_command_name(tag: &str) -> String {
    let stripped = tag
        .strip_prefix("CMD_REPLAY_UTILS_")
        .or_else(|| tag.strip_prefix("CMD_REPLAY_"))
        .or_else(|| tag.strip_prefix("CMD_FREE_CAMERA_"))
        .or_else(|| tag.strip_prefix("CMD_"))
        .or_else(|| tag.strip_prefix("MAP_"))
        .unwrap_or(tag);

    stripped
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.extend(chars.map(|c| c.to_ascii_lowercase()));
                    s
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
