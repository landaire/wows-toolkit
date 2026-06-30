//! Envoi automatique vers Discord (webhook) : video du rendu + tableau de stats.
//!
//! Modification interne (open-source MIT). Utilise le client HTTP bloquant deja
//! present dans le projet.

use std::path::Path;

const USER_AGENT: &str = "WoWsToolkit-Discord/1.0 (+https://github.com/landaire/wows-toolkit)";

/// Poste un message texte (ex. tableau de stats) sur le webhook.
pub fn post_text(client: &reqwest::blocking::Client, webhook: &str, message: &str) -> Result<(), String> {
    let body = serde_json::json!({ "content": message });
    let resp = client
        .post(webhook)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .json(&body)
        .send()
        .map_err(|e| format!("envoi texte: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Discord status {}", resp.status()))
    }
}

/// Poste un fichier (video) sur le webhook, avec un message optionnel.
pub fn post_file(
    client: &reqwest::blocking::Client,
    webhook: &str,
    file: &Path,
    message: &str,
) -> Result<(), String> {
    let nom = file
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video.mp4".to_string());
    let octets = std::fs::read(file).map_err(|e| format!("lecture video: {e}"))?;

    let payload = serde_json::json!({ "content": message }).to_string();
    let part = reqwest::blocking::multipart::Part::bytes(octets)
        .file_name(nom)
        .mime_str("video/mp4")
        .map_err(|e| format!("mime: {e}"))?;
    let form = reqwest::blocking::multipart::Form::new()
        .text("payload_json", payload)
        .part("files[0]", part);

    let resp = client
        .post(webhook)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .multipart(form)
        .send()
        .map_err(|e| format!("envoi video: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Discord status {} (video trop lourde ?)", resp.status()))
    }
}

// ---------- mise en forme du tableau de stats ----------

fn pad(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len > n {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    } else {
        let mut t = s.to_string();
        t.push_str(&" ".repeat(n - len));
        t
    }
}

fn rpad(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len >= n {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(n - len), s)
    }
}

fn fmt_num(v: Option<i64>) -> String {
    match v {
        None => "-".to_string(),
        Some(n) => {
            let s = n.abs().to_string();
            let mut out = String::new();
            for (i, c) in s.chars().rev().enumerate() {
                if i > 0 && i % 3 == 0 {
                    out.push(' ');
                }
                out.push(c);
            }
            let mut res: String = out.chars().rev().collect();
            if n < 0 {
                res.insert(0, '-');
            }
            res
        }
    }
}

fn as_i64(v: &serde_json::Value) -> Option<i64> {
    if v.is_null() {
        None
    } else if let Some(n) = v.as_i64() {
        Some(n)
    } else if let Some(f) = v.as_f64() {
        Some(f as i64)
    } else {
        None
    }
}

/// Construit le tableau equipe a partir du JSON de la partie (struct Match serialisee).
pub fn format_team_table(d: &serde_json::Value) -> String {
    let meta = &d["metadata"];
    let vehicles = d["vehicles"].as_array().cloned().unwrap_or_default();

    // equipe du joueur + resultat
    let mon_team = vehicles
        .iter()
        .find(|v| v["player"]["is_replay_perspective"].as_bool().unwrap_or(false))
        .and_then(|v| v["player"]["team_id"].as_i64());
    let br = &meta["battle_result"];
    let rtype = br["type"].as_str().unwrap_or("").to_lowercase();
    let gagnant = br["team_id"].as_i64();
    let (entete, emoji) = if rtype == "draw" || rtype == "tie" {
        ("MATCH NUL", "🤝")
    } else if let (Some(g), Some(m)) = (gagnant, mon_team) {
        if g == m { ("VICTOIRE", "🏆") } else { ("DÉFAITE", "💀") }
    } else if rtype == "win" || rtype == "victory" {
        ("VICTOIRE", "🏆")
    } else if rtype == "loss" || rtype == "defeat" {
        ("DÉFAITE", "💀")
    } else {
        ("RÉSULTAT", "⚔️")
    };

    let carte = meta["map"].as_str().unwrap_or("?");
    let mode = meta["game_mode"].as_str().unwrap_or("").replace('—', "-");
    let ts = meta["timestamp"].as_str().unwrap_or("");
    let date_str = if ts.len() >= 16 {
        format!("{}/{}/{} {}", &ts[8..10], &ts[5..7], &ts[0..4], &ts[11..16])
    } else {
        ts.to_string()
    };

    let a_pr = vehicles.iter().any(|v| !v["personal_rating"].is_null());

    let ligne = |v: &serde_json::Value| -> String {
        let clan = v["player"]["clan"].as_str().unwrap_or("");
        let nom_j = v["player"]["name"].as_str().unwrap_or("?");
        let nom = if clan.is_empty() { nom_j.to_string() } else { format!("[{clan}] {nom_j}") };
        let nav = format!(
            "{} T{}",
            v["name"].as_str().unwrap_or("?"),
            v["tier"].as_i64().unwrap_or(0)
        );
        let sr = &v["server_results"];
        let dmg = fmt_num(as_i64(&sr["damage"]));
        let kills = sr["kills"].as_i64().map(|k| k.to_string()).unwrap_or_else(|| "-".into());
        let spot = fmt_num(as_i64(&sr["spotting_damage"]));
        let mut s = format!(
            "{} {} {} {} {}",
            pad(&nom, 20), pad(&nav, 16), rpad(&dmg, 8), rpad(&kills, 3), rpad(&spot, 7)
        );
        if a_pr {
            let pr = as_i64(&v["personal_rating"]).map(|p| p.to_string()).unwrap_or_else(|| "-".into());
            s.push_str(&format!(" {}", rpad(&pr, 6)));
        }
        s
    };

    let entete_cols = {
        let mut s = format!("{} {} {} {} {}", pad("Joueur", 20), pad("Navire", 16), rpad("Dégâts", 8), rpad("Frg", 3), rpad("Repér.", 7));
        if a_pr {
            s.push_str(&format!(" {}", rpad("PR", 6)));
        }
        s
    };

    let dmg_of = |v: &serde_json::Value| as_i64(&v["server_results"]["damage"]).unwrap_or(0);
    let mut allies: Vec<&serde_json::Value> = vehicles
        .iter()
        .filter(|v| matches!(v["relation"].as_str(), Some("self") | Some("ally")))
        .collect();
    let mut ennemis: Vec<&serde_json::Value> = vehicles
        .iter()
        .filter(|v| v["relation"].as_str() == Some("enemy"))
        .collect();
    allies.sort_by_key(|v| std::cmp::Reverse(dmg_of(v)));
    ennemis.sort_by_key(|v| std::cmp::Reverse(dmg_of(v)));

    let mut out = String::new();
    out.push_str(&format!("{emoji} **{entete}** — {carte} · {mode}\n`{date_str}`\n```\n"));
    out.push_str("🟢 TON ÉQUIPE\n");
    out.push_str(&entete_cols);
    out.push('\n');
    for v in &allies {
        out.push_str(&ligne(v));
        out.push('\n');
    }
    out.push_str("\n🔴 ENNEMIS\n");
    out.push_str(&entete_cols);
    out.push('\n');
    for v in &ennemis {
        out.push_str(&ligne(v));
        out.push('\n');
    }
    out.push_str("```");
    if out.chars().count() > 1990 {
        out = out.chars().take(1980).collect::<String>() + "\n…```";
    }
    out
}
