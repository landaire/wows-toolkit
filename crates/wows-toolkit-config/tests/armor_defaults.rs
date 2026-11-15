use wows_toolkit_config::queries;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_config::queries::SavedLighting;

fn row(lighting: Option<SavedLighting>) -> ArmorViewerDefaultsRow {
    ArmorViewerDefaultsRow {
        show_plate_edges: true,
        show_waterline: false,
        show_zero_mm: true,
        armor_opacity: 0.5,
        waterline_opacity: 0.25,
        hull_opaque: true,
        hull_all_visible: false,
        armor_all_visible: true,
        show_splash_boxes: true,
        show_legend: false,
        legend_collapsed: true,
        legend_pos_x: Some(12.0),
        legend_pos_y: None,
        lighting,
    }
}

fn studio() -> SavedLighting {
    SavedLighting {
        enabled: true,
        flat_intensity: 0.3,
        flat_color: [0.9, 0.93, 1.0],
        key_intensity: 0.9,
        key_color: [1.0, 0.98, 0.95],
        azimuth_deg: 120.0,
        elevation_deg: 35.0,
        rim_strength: 0.3,
        rim_power: 4.0,
        specular_strength: 0.5,
        shininess: 48.0,
    }
}

#[tokio::test]
async fn saved_lighting_reads_back_as_saved() {
    let pool = wows_toolkit_config::test_pool().await;

    queries::save_armor_viewer_defaults(&pool, &row(Some(studio()))).await.expect("the row saves");
    let read = queries::get_armor_viewer_defaults(&pool).await.expect("the row reads").expect("a row was saved");

    assert_eq!(read.lighting, Some(studio()));
    assert!(read.show_splash_boxes && !read.show_waterline, "the other columns are kept");

    queries::save_armor_viewer_defaults(&pool, &row(None)).await.expect("the row saves again");
    let cleared = queries::get_armor_viewer_defaults(&pool).await.expect("the row reads").expect("a row was saved");
    assert_eq!(cleared.lighting, None, "a save without lighting clears it");
}

/// A row with no lighting column set has no lighting, rather than a default standing in for one.
#[tokio::test]
async fn a_row_saved_without_lighting_reads_as_no_lighting() {
    let pool = wows_toolkit_config::test_pool().await;
    sqlx::query("INSERT INTO armor_viewer_defaults (id, show_plate_edges) VALUES (1, 1)")
        .execute(&pool)
        .await
        .expect("an old row is written");

    let read = queries::get_armor_viewer_defaults(&pool).await.expect("the row reads").expect("the row is there");

    assert_eq!(read.lighting, None);
}
