//! The text the commands print for each analysis.

use std::io::Write;
use std::path::Path;

use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

use crate::audit::TypeAudit;
use crate::bench::BenchReport;
use crate::chat::ChatBody;
use crate::chat::ChatEvent;
use crate::parse::Diagnostic;
use crate::query::PlayerRow;
use crate::query::Roster;
use crate::search::SearchHit;
use crate::search::parse_date_time;
use crate::summary::ReplaySummary;
use crate::survey::ReplaySurvey;
use crate::survey::SurveyTotals;

/// How many audit notes the survey shows per replay.
const AUDITS_SHOWN: usize = 10;

/// How many of the newest replays `search` lists.
const SEARCH_SHOWN: usize = 10;

/// To stderr: payloads the parser did not fully consume.
pub fn diagnostics(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }

    eprintln!("payload validation: {} property payload(s) under-consumed", diagnostics.len());
    for d in diagnostics {
        eprintln!("  {} (def {:?}): consumed {} of {} bytes", d.context, d.semantic_name, d.consumed, d.payload_len);
    }
}

pub fn spec(specs: &[EntitySpec]) {
    println!("Have {} entities", specs.len());
    for entity in specs.iter() {
        println!();
        println!(
            "{} has {} properties ({} internal) and {}/{}/{} base/cell/client methods",
            entity.name,
            entity.properties.len(),
            entity.internal_properties.len(),
            entity.base_methods.len(),
            entity.cell_methods.len(),
            entity.client_methods.len()
        );

        println!("Properties:");
        for (i, property) in entity.properties.iter().enumerate() {
            println!(" - {}: {} flag={:?} type={:?}", i, property.name, property.flags, property.prop_type);
        }
        println!("Internal properties:");
        for (i, property) in entity.internal_properties.iter().enumerate() {
            println!(" - {}: {} type={:?}", i, property.name, property.prop_type);
        }
        println!("Client methods:");
        for (i, method) in entity.client_methods.iter().enumerate() {
            println!(" - {}: {}", i, method.name);
            for arg in method.args.iter() {
                println!("      - {:?}", arg);
            }
        }
    }
}

pub fn type_audit(audit: &TypeAudit) {
    println!("{:<40} {:<10} RUST TYPE", "DEF NAME", "NEWTYPE?");
    for name in &audit.names {
        match name.newtype {
            Some(newtype) => println!("{:<40} {:<10} {}", name.def_name, "yes", newtype),
            None => println!("{:<40} {:<10} -", name.def_name, "no"),
        }
    }
    println!("\n{} of {} semantic names have a domain newtype", audit.covered(), audit.names.len());
}

pub fn summary(summary: &ReplaySummary) {
    println!("Username: {}", summary.player_name);
    println!("Date/time: {}", summary.date_time);
    println!("Map: {}", summary.map);
    println!("Vehicle: {}", summary.vehicle);
    println!("Game mode: {} {:?}", summary.game_mode, summary.game_logic);
    println!("Game version: {}", summary.client_version);
    println!();

    for (ribbon, count) in &summary.stats.ribbons {
        println!("{ribbon:?}: {count}");
    }
    println!();

    println!("Total damage: {:.0}", summary.stats.enemy_damage);
}

pub fn chat(events: &[ChatEvent]) {
    for event in events {
        let sender = event.sender.as_deref().unwrap_or("<UNKNOWN_USERNAME>");
        let clock = event.clock;
        match &event.body {
            ChatBody::Chat { audience, message } => println!("{clock}: {sender}: {audience} {message}"),
            ChatBody::VoiceLine(message) => println!("{clock}: {sender}: voiceline {message:#?}"),
        }
    }
}

/// The start of a replay's survey line, shown before it is parsed.
pub fn survey_start(file_name: &str) {
    print!("Parsing {}: ", truncate_string(file_name, 20));

    // A failed flush only delays the line.
    let _ = std::io::stdout().flush();
}

pub fn survey_parsed(survey: &ReplaySurvey) {
    if survey.invalid_packets > 0 {
        println!("OK ({} packets, {} invalid)", survey.total_packets, survey.invalid_packets);
    } else {
        println!("OK ({} packets)", survey.total_packets);
    }
}

pub fn survey_totals(totals: &SurveyTotals) {
    let mut audited: Vec<_> = totals.audits.iter().collect();
    audited.sort_by_cached_key(|(_, replay)| parse_date_time(&replay.date_time).ok());
    for (file_name, replay) in audited {
        println!();
        println!("{} ({}) has {} audits:", truncate_string(file_name, 20), replay.date_time, replay.audits.len());
        for (cnt, audit) in replay.audits.iter().enumerate() {
            if cnt >= AUDITS_SHOWN {
                println!("...truncating");
                break;
            }
            println!(" - {}", audit);
        }
    }

    let percent = |part: usize, whole: usize| 100. * part as f64 / whole as f64;
    println!();
    println!("Found {} replay files", totals.total);
    println!("- {} ({:.0}%) were parsed", totals.successes, percent(totals.successes, totals.total));
    println!(
        "  - Of which {} ({:.0}%) contained invalid packets",
        totals.successes_with_invalids,
        percent(totals.successes_with_invalids, totals.successes)
    );
    println!("- {} ({:.0}%) had a parse error", totals.parse_failures, percent(totals.parse_failures, totals.total));
    println!(
        "- {} ({:.0}%) are an unrecognized version",
        totals.version_failures,
        percent(totals.version_failures, totals.total)
    );
    for (version, count) in &totals.invalid_versions {
        println!("  - Version {} appeared {} times", version, count);
    }
}

pub fn bench(report: &BenchReport) {
    if let Some(preload) = &report.preload {
        println!("preloaded {} metadata blobs in {:?}", preload.blobs, preload.elapsed);
    }

    let per_parse_us = report.per_parse().map_or(0.0, |per_parse| per_parse.as_secs_f64() * 1e6);
    let mib_s = report.throughput_mib_s().unwrap_or(0.0);
    println!(
        "parsed={} failed={} bytes={} elapsed={:?} per_parse={:.1}us throughput={:.1}MiB/s sink={}",
        report.parsed, report.failed, report.bytes, report.elapsed, per_parse_us, mib_s, report.sink
    );
}

/// The newest replays, newest last, after any whose time does not parse.
pub fn search(hits: &[SearchHit]) {
    for hit in hits {
        if let Err(e) = parse_date_time(&hit.date_time) {
            println!("Couldn't parse '{}' because {:?}", hit.date_time, e);
        }
    }

    println!("Found {} games", hits.len());
    for hit in hits.iter().rev().take(SEARCH_SHOWN) {
        println!("{:?} {} {} {} {}", hit.path, hit.player_name, hit.date_time, hit.map, hit.vehicle);
    }
}

/// `major.minor.patch.build`.
pub fn version(version: Version) -> String {
    let build = version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string());

    format!("{}.{}.{}.{}", version.major, version.minor, version.patch, build)
}

pub fn players(rows: &[PlayerRow], as_json: bool) {
    if as_json {
        for r in rows {
            // The ship's index stands in for a name the build lacks.
            let value = serde_json::json!({
                "player_name": r.player_name,
                "entity_id": r.entity_id,
                "avatar_id": r.avatar_id,
                "account_id": r.account_id,
                "ship_id": r.ship_id,
                "ship_index": r.ship_index,
                "ship_name": r.ship_label(),
                "team_id": r.team_id,
                "relation": r.relation.name(),
                "is_bot": r.is_bot,
            });
            println!("{value}");
        }
        return;
    }

    println!("{:<9} {:<20} {:<4} {:<6} {:<22} {:<11} BOT", "ENTITY", "PLAYER", "TEAM", "REL", "SHIP", "SHIP_ID");
    for r in rows {
        println!(
            "{:<9} {:<20} {:<4} {:<6} {:<22} {:<11} {}",
            r.entity_id.raw(),
            truncate_string(&r.player_name, 20),
            r.team_id.raw(),
            r.relation.name(),
            truncate_string(r.ship_label(), 22),
            r.ship_id.raw(),
            if r.is_bot { "bot" } else { "" }
        );
    }
}

pub fn roster(path: &Path, roster: &Roster, as_json: bool) {
    if as_json {
        let value = serde_json::json!({
            "path": path.display().to_string(),
            "version": roster.version,
            "own_class": roster.own_class,
            "classes": roster.classes,
        });
        println!("{value}");
        return;
    }

    let rendered: Vec<String> = roster.classes.iter().map(|(class, count)| format!("{class}={count}")).collect();
    println!(
        "{}\t{}\t{}\t{}",
        roster.version,
        roster.own_class.as_deref().unwrap_or("-"),
        rendered.join(","),
        path.display()
    );
}

fn truncate_string(s: &str, length: usize) -> &str {
    match s.char_indices().nth(length) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_characters_not_bytes() {
        assert_eq!(truncate_string("abc", 5), "abc");
        assert_eq!(truncate_string("\u{e9}t\u{e9}s", 2), "\u{e9}t");
    }
}
