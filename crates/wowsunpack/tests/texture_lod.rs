//! Texture tier selection against a real install. Ignored: needs an install.
//!
//! WoWs splits a texture into a `.dds` mip tail plus up to three single-mip
//! files above it (`.dd2`, `.dd1`, `.dd0`), each double the one below. These
//! tests pin the tier and mip a budget resolves to, since picking a tier too
//! high is the whole cost the budget exists to avoid.

use std::io::Cursor;
use std::path::PathBuf;

use wowsunpack::export::texture::MaxEdge;
use wowsunpack::export::texture::TextureLod;
use wowsunpack::export::texture::TierDrop;
use wowsunpack::export::texture::load_dds_from_vfs;

/// A four-tier texture: 4096 `.dd0`, 2048 `.dd1`, 1024 `.dd2`, 512 `.dds`.
const FOUR_TIER: &str = "content/gameplay/japan/ship/battleship/textures/JSB039_Yamato_1945_Hull_a.dds";

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn vfs() -> vfs::VfsPath {
    wowsunpack::game_data::build_game_vfs(&game_dir()).expect("game vfs")
}

fn capped(pixels: u32) -> TextureLod {
    TextureLod::Capped(MaxEdge::new(pixels).expect("nonzero budget"))
}

/// Top-mip edge and mip count of a loaded DDS buffer.
fn header(bytes: &[u8]) -> (u32, u32) {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(bytes)).expect("dds header");
    (dds.get_width().max(dds.get_height()), dds.get_num_mipmap_levels().max(1))
}

#[test]
#[ignore = "requires a World of Warships install"]
fn budget_picks_the_largest_tier_that_fits() {
    let vfs = vfs();
    // Each budget must land on its own tier, never a larger one, and never fall
    // all the way back to the tail when a mid tier would serve.
    for (budget, want_edge) in [(4096, 4096), (2048, 2048), (1024, 1024), (512, 512)] {
        let bytes = load_dds_from_vfs(&vfs, FOUR_TIER, capped(budget)).expect("tier loaded");
        let (edge, _) = header(&bytes);
        assert_eq!(edge, want_edge, "budget {budget} should read the {want_edge} tier");
    }
}

#[test]
#[ignore = "requires a World of Warships install"]
fn full_detail_reads_the_top_tier() {
    let bytes = load_dds_from_vfs(&vfs(), FOUR_TIER, TextureLod::Full).expect("top tier");
    assert_eq!(header(&bytes).0, 4096);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn budget_between_tiers_rounds_down_to_the_smaller_one() {
    // 1500 sits between the 1024 and 2048 tiers. Taking 2048 would exceed the
    // budget, so the 1024 tier is the only correct choice.
    let bytes = load_dds_from_vfs(&vfs(), FOUR_TIER, capped(1500)).expect("tier loaded");
    assert_eq!(header(&bytes).0, 1024);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn budget_below_the_tail_stays_in_the_tail_and_uses_its_mip_chain() {
    // Under the tail's own top mip there is no smaller file to read: the tail's
    // chain has to serve it, so the loader must not read a larger tier.
    let bytes = load_dds_from_vfs(&vfs(), FOUR_TIER, capped(128)).expect("tail loaded");
    let (edge, mips) = header(&bytes);
    assert_eq!(edge, 512, "a sub-tail budget must still read the tail");
    assert!(mips > 1, "the tail carries the mip chain that serves sub-tail budgets");

    // The decode resolves the rest: a 128 budget yields a 128 image.
    let png = wowsunpack::export::texture::dds_to_png(&bytes, capped(128)).expect("decode");
    let img = image_dds::image::load_from_memory(&png).expect("png").into_rgba8();
    assert_eq!(img.width().max(img.height()), 128);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn decode_honours_the_budget_for_every_tier() {
    let vfs = vfs();
    for budget in [4096, 2048, 1024, 512, 256, 64] {
        let bytes = load_dds_from_vfs(&vfs, FOUR_TIER, capped(budget)).expect("tier loaded");
        let png = wowsunpack::export::texture::dds_to_png(&bytes, capped(budget)).expect("decode");
        let img = image_dds::image::load_from_memory(&png).expect("png").into_rgba8();
        assert!(
            img.width().max(img.height()) <= budget,
            "budget {budget} produced a {}x{} image",
            img.width(),
            img.height()
        );
    }
}

#[test]
#[ignore = "requires a World of Warships install"]
fn mesh_lod_steps_down_one_tier_per_level() {
    let vfs = vfs();
    // FOUR_TIER's ladder is 4096 / 2048 / 1024 / 512, so mesh LOD k lands on
    // the k-th entry.
    for (lod, want_edge) in [(0, 4096), (1, 2048), (2, 1024), (3, 512)] {
        let bytes = load_dds_from_vfs(&vfs, FOUR_TIER, TextureLod::from_mesh_lod(lod)).expect("tier loaded");
        assert_eq!(header(&bytes).0, want_edge, "mesh LOD {lod} should take the {want_edge} tier");
    }
}

#[test]
#[ignore = "requires a World of Warships install"]
fn mesh_lod_past_the_ladder_clamps_to_the_tail() {
    // The tail is the lowest separately authored level; a rank beyond it must
    // clamp rather than fail or read something larger.
    let bytes = load_dds_from_vfs(&vfs(), FOUR_TIER, TextureLod::from_mesh_lod(9)).expect("tail loaded");
    assert_eq!(header(&bytes).0, 512);
}

#[test]
fn mesh_lod_zero_is_full_detail() {
    assert_eq!(TextureLod::from_mesh_lod(0), TextureLod::Full);
    assert_ne!(TextureLod::from_mesh_lod(1), TextureLod::Full);
}

#[test]
fn tier_drop_rejects_zero() {
    assert!(TierDrop::new(0).is_none());
    assert_eq!(TierDrop::new(2).map(TierDrop::tiers), Some(2));
}

#[test]
fn max_edge_rejects_zero() {
    assert!(MaxEdge::new(0).is_none());
    assert_eq!(MaxEdge::new(512).map(MaxEdge::pixels), Some(512));
}

#[test]
fn from_max_edge_treats_absent_and_zero_as_full_detail() {
    assert_eq!(TextureLod::from_max_edge(None), TextureLod::Full);
    assert_eq!(TextureLod::from_max_edge(Some(0)), TextureLod::Full);
    assert_eq!(TextureLod::from_max_edge(Some(1024)), capped(1024));
}
