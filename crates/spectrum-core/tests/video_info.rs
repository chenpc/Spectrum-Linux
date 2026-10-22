//! ISO-BMFF video metadata: sample-entry codec fourcc + `mvhd` duration.

use std::path::PathBuf;

use spectrum_core::{mp4, scan};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn synthetic_mp4_reports_codec_and_duration() {
    let dir = temp_dir("video_info");
    let path = fixture::write_mp4_fixture(&dir, "clip.mp4", 9, 18, 0, true);
    let data = std::fs::read(&path).unwrap();
    let info = mp4::parse_video_info(&data).unwrap();
    assert_eq!(info.codec.as_deref(), Some("avc1"));
    assert_eq!(info.duration_secs, Some(12.5));
    assert_eq!(info.describe(), "avc1 · 12.5s");
}

/// The real FFmpeg-written fixture is the case that broke the naive "stsd is a full box" assumption:
/// its stsd content starts with entry_count (no version/flags), so the entry_count is at offset 0.
#[test]
fn real_ffmpeg_fixture_uses_plain_box_layout() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hlg_video.mp4");
    let data = std::fs::read(&path).unwrap();
    let info = mp4::parse_video_info(&data).unwrap();
    assert_eq!(info.codec.as_deref(), Some("hvc1"));
    // mvhd version 0: timescale at content[12], duration at content[16] -> 1000 / 251 = 0.251s
    // (mdhd agrees: 60000 / 15015 = 0.25025s, so this fixture really is a quarter-second clip)
    assert_eq!(info.duration_secs, Some(0.251));
    assert_eq!(info.describe(), "hvc1 · 0.3s");
}

#[test]
fn duration_format_switches_at_one_minute() {
    let i = |d: f64| mp4::VideoInfo {
        codec: None,
        duration_secs: Some(d),
    };
    assert_eq!(i(59.94).describe(), "59.9s");
    assert_eq!(i(60.0).describe(), "01:00");
    assert_eq!(i(75.0).describe(), "01:15");
    assert_eq!(
        mp4::VideoInfo {
            codec: Some("avc1".to_string()),
            duration_secs: None,
        }
        .describe(),
        "avc1"
    );
    assert_eq!(mp4::VideoInfo::default().describe(), "");
}

#[test]
fn scan_populates_video_only_for_videos() {
    let dir = temp_dir("video_scan");
    fixture::write_mp4_fixture(&dir, "a.mp4", 9, 18, 0, true);
    fixture::write_jpeg_fixture(&dir, "b.jpg", None, Some(1), None, 1);

    let entries = scan::scan_folder(&dir);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].kind, scan::MediaKind::Video);
    assert_eq!(
        entries[0].video.as_ref().unwrap().codec.as_deref(),
        Some("avc1")
    );
    assert!(entries[1].video.is_none());
}
