//! Stage benchmarks for the replay parse pipeline.
//!
//! Four groups, chosen so that time can be attributed rather than guessed:
//!
//! - `container`   Blowfish decrypt, zlib inflate, metadata JSON.
//! - `parse_only`  the `parse_packet` loop with no world attached.
//! - `parse_and_process`  the same loop feeding `BattleWorld`. Subtracting
//!   `parse_only` gives the decode-and-ingest cost.
//! - `into_report` report assembly, from a world parsed in the setup step.
//!
//! Needs local game data; see `benches/support/mod.rs` for the environment
//! variables. With no resolvable replay the benchmark reports nothing and
//! exits cleanly, which is what happens in CI.

use criterion::BatchSize;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use std::hint::black_box;
use wows_battle_world::world::BattleWorld;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::packet2::Parser;

mod support;

use support::Case;

/// A world configured the way `Replay::parse` configures it: fire-chance
/// analysis needs the whole-match hit history and the salvo log, both off by
/// default.
fn new_world<'a>(case: &'a Case) -> BattleWorld<'a, 'a, wowsunpack::game_params::provider::GameMetadataProvider> {
    let mut world = BattleWorld::new(&case.replay.meta, &*case.provider, Some(&*case.constants));
    world.set_record_hit_history(true);
    world.set_record_salvo_history(true);
    world
}

fn drive(
    case: &Case,
    world: Option<&mut BattleWorld<'_, '_, wowsunpack::game_params::provider::GameMetadataProvider>>,
) {
    let mut parser = Parser::with_version(&case.specs, case.version);
    let mut remaining = case.replay.packet_data();
    match world {
        Some(world) => {
            while !remaining.is_empty() {
                match parser.parse_packet(&mut remaining) {
                    Ok(packet) => world.process(&packet),
                    Err(_) => break,
                }
            }
        }
        None => {
            while !remaining.is_empty() {
                match parser.parse_packet(&mut remaining) {
                    Ok(packet) => {
                        black_box(&packet);
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

fn bench(c: &mut Criterion) {
    let cases = support::cases();
    if cases.is_empty() {
        eprintln!("no benchmark cases resolved; skipping");
        return;
    }

    let mut container = c.benchmark_group("container");
    for case in &cases {
        container.throughput(Throughput::Bytes(case.bytes.len() as u64));
        container.bench_function(&case.name, |b| {
            b.iter(|| black_box(ReplayFile::from_bytes(black_box(&case.bytes)).expect("case parsed at load")))
        });
    }
    container.finish();

    let mut parse_only = c.benchmark_group("parse_only");
    for case in &cases {
        parse_only.throughput(Throughput::Bytes(case.replay.packet_data().len() as u64));
        parse_only.bench_function(&case.name, |b| b.iter(|| drive(case, None)));
    }
    parse_only.finish();

    let mut full = c.benchmark_group("parse_and_process");
    for case in &cases {
        full.throughput(Throughput::Bytes(case.replay.packet_data().len() as u64));
        full.bench_function(&case.name, |b| {
            b.iter_batched_ref(|| new_world(case), |world| drive(case, Some(world)), BatchSize::LargeInput)
        });
    }
    full.finish();

    let mut report = c.benchmark_group("into_report");
    for case in &cases {
        report.bench_function(&case.name, |b| {
            b.iter_batched(
                || {
                    let mut world = new_world(case);
                    drive(case, Some(&mut world));
                    world.finish();
                    world
                },
                |world| black_box(world.into_report()),
                BatchSize::LargeInput,
            )
        });
    }
    report.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
