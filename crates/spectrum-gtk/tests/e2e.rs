use gdk4 as gdk;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use glib as glib;
use glib::translate::ToGlibPtr;
use graphene as graphene;
use gtk4 as gtk;
use gtk4::prelude::*;
use spectrum_core::crop::CropRect;
use spectrum_core::edit::EditOp;
use spectrum_core::clipboard::{ClipboardMode};
use spectrum_testkit::fixture;
use spectrum_core::nav::Direction;
use spectrum_core::select::ClickMods;
use spectrum_gtk::app::{
    open_folder, rebuild_subfolders, record_click, refresh, rescan_import, set_clipboard_from_selection, App,
};

fn selected_indices(ui: &spectrum_gtk::app::Ui) -> Vec<usize> {
    (0..*ui.item_count.borrow()).filter(|&i| ui.selection.is_selected(i as u32)).collect()
}

fn fixture_dir() -> PathBuf {
    let dir = PathBuf::from("/tmp/spectrum-e2e-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    fixture::write_fixture(&dir, "hlg_photo.heic", 9, 18, 0, true);
    fixture::write_fixture(&dir, "sdr_photo.heic", 1, 13, 0, false);
    // JPEG signalling comes from EXIF/ICC, not a colr box.
    fixture::write_jpeg_fixture(&dir, "hlg_icc.jpg", Some("Rec. ITU-R BT.2100 HLG"), Some(1), None, 1);
    // ICC name has no BT.2100 marker; only the `cicp` tag says HLG.
    fixture::write_jpeg_fixture_exif(
        &dir,
        "cicp_hlg.jpg",
        Some("Display P3"),
        &fixture::ExifFixture {
            orientation: 1,
            colorspace: Some(1),
            cicp: Some((9, 18, 0, 0)),
..fixture::ExifFixture::default()
},
    );
    // HEIF whose nclx is "unspecified": only the ICC profile's cicp tag says HLG.
    fixture::write_heif_fixture_with_icc(
        &dir,
        "heif_icc_cicp.heic",
        2,
        2,
        0,
        false,
        Some("Display P3"),
        Some((9, 18, 0, 0)),
        None,
    );
    fixture::write_jpeg_fixture(&dir, "sdr_exif.jpg", None, Some(1), None, 1);
    // Pixels are HLG but the ICC name claims sRGB -> must not be flagged HDR.
    fixture::write_jpeg_fixture(&dir, "mislabeled.jpg", Some("sRGB IEC61966-2.1"), Some(1), None, 1);
    // EXIF CustomRendered == 3 is the gain-map signal -> badge "HDR".
    fixture::write_jpeg_fixture(&dir, "gainmap.jpg", None, Some(1), Some(3), 1);
    // ISO 21496-1 APP2 gain map (the reference's first choice) -> badge "HDR".
    fixture::write_jpeg_fixture_exif(
        &dir,
        "app2_gainmap.jpg",
        None,
        &fixture::ExifFixture {
            orientation: 1,
            colorspace: Some(1),
            custom_rendered: None,
            make: None,
            model: None,
            iso: None,
            exposure: None,
            f_number: None,
            focal_length: None,
            gain_map: true,
        cicp: None,
        dmnd: None,
    date_time_original: None,
},
    );
    fixture::write_mp4_fixture(&dir, "hlg_video.mp4", 9, 18, 0, true);
    // glycin cannot decode the box-only/header-only fixtures (no pixel data), so add one fixture it
    // can decode; that is what makes thumbnails render in the snapshot.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.png");
    std::fs::copy(&src, dir.join("sample.png")).unwrap();
    dir
}

fn text_of(l: &gtk::Label) -> String {
    l.text().to_string()
}

fn row_label_text(list: &gtk::ListBox, i: i32) -> String {
    list.row_at_index(i)
        .and_then(|r| r.child().and_then(|c| c.downcast::<gtk::Box>().ok()))
        .and_then(|b| b.last_child().and_then(|c| c.downcast::<gtk::Label>().ok()))
        .map(|l| text_of(&l))
        .unwrap_or_default()
}

fn plain_row_text(list: &gtk::ListBox, i: i32) -> String {
    list.row_at_index(i)
        .and_then(|r| r.child().and_then(|c| c.downcast::<gtk::Label>().ok()))
        .map(|l| text_of(&l))
        .unwrap_or_default()
}

fn list_row_count(list: &gtk::ListBox) -> i32 {
    let mut n = 0;
    while list.row_at_index(n).is_some() {
        n += 1;
    }
    n
}

fn row_names(list: &gtk::ListBox) -> Vec<String> {
    (0..3)
        .map(|i| row_label_text(list, i).split("  ·  ").next().unwrap_or("").to_string())
        .collect()
}

fn pump(sleep_ms: u64) {
    let ctx = glib::MainContext::default();
    if !ctx.iteration(false) {
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }
}

/// Pump until the window's content box snapshot produces a node.
///
/// A `GtkPicture` only contributes a render node after a real paint pass: its snapshot cache is
/// built during rendering, so a picture painted while empty (no paintable) keeps an empty cache
/// until the next frame. Allocation alone is not enough — `iteration(false)` must also let the
/// frame clock advance. `snapshot_child` requires the child to be a direct child of the widget.
/// glycin fills the detail picture asynchronously and the window grows to fit it, so a snapshot is
/// only comparable across runs once the paintable is set AND both the picture and the window size
/// stop changing. Polling only the picture size could settle on the pre-load size.
/// A snapshot test must keep the search entry from ever drawing a caret: a focused entry can show a
/// caret even when it is non-editable, and that made the baseline unstable in ~1 of 12 full-suite
/// runs (19 differing px at x=9, y=46..64). Non-editable + non-focusable removes it entirely.
/// Snapshot tests must have nothing in the focus chain: a focused widget draws a focus ring and an
/// entry draws a blinking caret, both of which make the baseline unstable (observed: 2232 px
/// differing over y=39..72 when the entry gained focus, and a 19 px caret column at x=9).
fn freeze_focus(w: &gtk::Widget) {
    w.set_property("focusable", &false);
    let mut child = w.first_child();
    while let Some(c) = child {
        freeze_focus(&c);
        child = c.next_sibling();
    }
}

fn freeze_search(ui: &spectrum_gtk::app::Ui) {
    ui.search.set_editable(false);
    let w: gtk::Widget = ui.search.clone().upcast();
    w.set_property("focusable", &false);
    // keep the entry out of the focus chain so no caret is drawn
}

fn settle_picture(picture: &gtk::Picture, window: &gtk::Window) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while picture.paintable().is_none() && Instant::now() < end {
        pump(20);
    }
    if picture.paintable().is_none() {
        return false;
    }
    let mut last = (0, 0, 0, 0);
    let mut stable = 0;
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        let cur = (picture.width(), picture.height(), window.width(), window.height());
        if cur.0 > 0 && cur.2 > 0 && cur == last {
            stable += 1;
            if stable >= 10 {
                return true;
            }
        } else {
            stable = 0;
            last = cur;
        }
        pump(20);
    }
    false
}

fn settle_snapshot(window: &gtk::Window, deadline: Duration) -> bool {
    let end = Instant::now() + deadline;
    loop {
        let Some(content) = window.child() else { return false };
        let snap = gtk::Snapshot::new();
        window.snapshot_child(&content, &snap);
        if snap.to_node().is_some() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        pump(20);
    }
}

/// Pump until the list has `want` rows built by the item factory and at least one row thumbnail has a
/// paintable. Rows are created lazily on the next main-loop pass after the model is filled, so a
/// snapshot taken right after `App::new` shows the sidebar chrome but no rows.
fn settle_rows(list: &gtk::ListBox, want: usize, deadline: Duration) -> bool {
    let end = Instant::now() + deadline;
    loop {
        let mut n: usize = 0;
        let mut painted = false;
        while let Some(row) = list.row_at_index(n as i32) {
            n += 1;
            if row
                .child()
                .and_then(|c| c.downcast::<gtk::Box>().ok())
                .and_then(|b| b.first_child().and_then(|c| c.downcast::<gtk::Picture>().ok()))
                .map(|p| p.paintable().is_some())
                .unwrap_or(false)
            {
                painted = true;
            }
        }
        if n >= want && painted {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        pump(20);
    }
}

fn snapshot_png(window: &gtk::Window, out: &Path) -> Result<(), String> {
    let content = window.child().ok_or("window has no child")?;
    let snap = gtk::Snapshot::new();
    window.snapshot_child(&content, &snap);
    let node = snap.to_node().ok_or("snapshot produced no node")?;
    let native = window.native().ok_or("window not realized")?;
    let renderer = native.renderer().ok_or("no renderer")?;
    let rect = graphene::Rect::new(0.0, 0.0, content.width() as f32, content.height() as f32);
    let tex = renderer.render_texture(&node, Some(&rect));
    tex.save_to_png(out).map_err(|e| e.to_string())
}

// Count pixels of the red test thumbnail in the rendered snapshot, restricted to x < max_x so the
// check proves the sidebar list actually rendered a thumbnail (not just the detail picture).
/// Compare two PNGs and describe where they differ, so an unstable snapshot reports the region
/// (caret, highlight, thumbnail) instead of only a byte count.
fn diff_bbox(a: &Path, b: &Path) -> String {
    let pa = match gdk_pixbuf::Pixbuf::from_file(a) {
        Ok(p) => p,
        Err(e) => return format!("unreadable {e}"),
    };
    let pb = match gdk_pixbuf::Pixbuf::from_file(b) {
        Ok(p) => p,
        Err(e) => return format!("unreadable {e}"),
    };
    if pa.width() != pb.width() || pa.height() != pb.height() {
        return format!("size {}x{} vs {}x{}", pa.width(), pa.height(), pb.width(), pb.height());
    }
    let (w, h) = (pa.width() as usize, pa.height() as usize);
    let ch = pa.n_channels() as usize;
    let sa = pa.rowstride() as usize;
    let sb = pb.rowstride() as usize;
    let da = unsafe { gdk_pixbuf::Pixbuf::pixels(&pa) };
    let db = unsafe { gdk_pixbuf::Pixbuf::pixels(&pb) };
    let mut n = 0usize;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let ia = y * sa + x * ch;
            let ib = y * sb + x * ch;
            if da[ia..ia + ch] != db[ib..ib + ch] {
                n += 1;
                if x < x0 { x0 = x; }
                if x > x1 { x1 = x; }
                if y < y0 { y0 = y; }
                if y > y1 { y1 = y; }
            }
        }
    }
    if n == 0 {
        return "no pixel difference".to_string();
    }
    format!("{n} px differ, bbox x={x0}..{x1} y={y0}..{y1}")
}

fn red_pixels_total(path: &Path) -> usize {
    red_pixels(path, i32::MAX)
}

fn red_pixels(path: &Path, max_x: i32) -> usize {
    let pb = match gdk_pixbuf::Pixbuf::from_file(path) {
        Ok(pb) => pb,
        Err(_) => return 0,
    };
    let px = unsafe { gdk_pixbuf::Pixbuf::pixels(&pb) };
    let ch = pb.n_channels() as usize;
    let rowstride = pb.rowstride() as usize;
    let mut n = 0;
    for y in 0..pb.height() as usize {
        for x in 0..pb.width() as usize {
            if x as i32 >= max_x {
                break;
            }
            let o = y * rowstride + x * ch;
            if px[o] == 255 && px[o + 1] == 0 && px[o + 2] == 0 {
                n += 1;
            }
        }
    }
    n
}

// GTK must be initialized on the main thread; `test_synced` runs the body on GTK's
// exclusive thread so several tests can share one GTK init.
#[test]
fn e2e_gtk_list_and_grid_select_and_hdr_toggle() {
    let problems = gtk4::test_synced(|| {
        let app = App::new(&fixture_dir());
        app.window.present();

        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(5)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let model = app.ui.selection.model().ok_or("no model".to_string()).unwrap();
        if model.n_items() != 11 {
            problems.push(format!("model items = {}", model.n_items()));
            return problems;
        }

        // list rows: thumbnail picture + "name · transfer function" label.
        // scan_folder sorts by path, so this order is fixed.
        let mut rows = 0;
        while app.ui.list.row_at_index(rows).is_some() {
            rows += 1;
        }
        if rows != 11 {
            problems.push(format!("list rows = {rows}"));
            return problems;
        }
        let want = [
            (0, "app2_gainmap.jpg", "HDR"),
            (1, "cicp_hlg.jpg", "HLG"),
            (2, "gainmap.jpg", "HDR"),
            (3, "heif_icc_cicp.heic", "HLG"),
            (4, "hlg_icc.jpg", "HLG"),
            (5, "hlg_photo.heic", "HLG"),
            (6, "hlg_video.mp4", "HLG"),
            (7, "mislabeled.jpg", "SDR"),
            (8, "sample.png", "SDR"),
            (9, "sdr_exif.jpg", "SDR"),
            (10, "sdr_photo.heic", "SDR"),
        ];
        for (i, name, transfer) in want {
            let row = app.ui.list.row_at_index(i).unwrap();
            let Some(row_box) = row.child().and_then(|c| c.downcast::<gtk::Box>().ok()) else {
                problems.push(format!("row {i} has no thumbnail+label box"));
                continue;
            };
            if row_box.first_child().and_then(|c| c.downcast::<gtk::Picture>().ok()).is_none() {
                problems.push(format!("row {i} has no thumbnail picture"));
            }
            let label = row_box.last_child().and_then(|c| c.downcast::<gtk::Label>().ok());
            if label.is_none() {
                problems.push(format!("row {i} has no label"));
                continue;
            }
            let t = text_of(&label.unwrap());
            if t != format!("{name}  ·  {transfer}") {
                problems.push(format!("row {i} label = {t}"));
            }
        }

        // the decodable fixture must produce a real thumbnail texture (not an empty placeholder)
        let row3 = app.ui.list.row_at_index(8).unwrap();
        let thumb = row3
            .child()
            .and_then(|c| c.downcast::<gtk::Box>().ok())
            .and_then(|b| b.first_child().and_then(|c| c.downcast::<gtk::Picture>().ok()));
        match thumb {
            Some(p) if p.paintable().is_none() => problems.push("row 8 thumbnail has no paintable".to_string()),
            None => problems.push("row 8 has no thumbnail picture".to_string()),
            Some(_) => {}
        }

        // the grid must be driven by the same selection model as the list
        let grid_model = app.ui.grid.model().map(|m| m.upcast::<glib::Object>());
        if grid_model != Some(app.ui.selection.clone().upcast::<glib::Object>()) {
            problems.push("grid is not bound to the shared selection model".to_string());
        }
        if app.ui.grid.factory().is_none() {
            problems.push("grid has no item factory".to_string());
        }
        if app.ui.grid.min_columns() != 3 || app.ui.grid.max_columns() != 6 {
            problems.push(format!(
                "grid columns = {}..{}",
                app.ui.grid.min_columns(),
                app.ui.grid.max_columns()
            ));
        }

        // the list and the grid must share one thumbnail cache: 6 files decoded once, then reused
        {
            let svc = app.ui.thumbs.borrow_mut();
            if svc.max() != 64 {
                problems.push(format!("thumbnail cap = {}", svc.max()));
            }
            if svc.decodes() != 1 {
                problems.push(format!("decodes = {} (expected 1: only sample.png has pixel data)", svc.decodes()));
            }
            if svc.hits() == 0 {
                problems.push("grid did not reuse the thumbnail cache".to_string());
            }
            if svc.len() != 1 {
                problems.push(format!("cache entries = {}", svc.len()));
            }
        }
        // the 8x6 fixture is smaller than the cap, so it must not be upscaled
        let sample = PathBuf::from("/tmp/spectrum-e2e-fixtures/sample.png");
        match app.ui.thumbs.borrow_mut().get(&sample) {
            Ok((_, t)) => {
                if !t.cached {
                    problems.push("sample.png was decoded a second time".to_string());
                }
                if (t.width, t.height) != (8, 6) {
                    problems.push(format!("sample.png thumbnail = {}x{}", t.width, t.height));
                }
            }
            Err(e) => problems.push(format!("sample.png not cached: {e}")),
        }

        // selecting an item drives the shared selection model -> detail + badge.
        // Start from a different index than the initial selection so the notify actually fires.
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        if text_of(&app.ui.badge) != "HDR" {
            problems.push(format!("after select 0: badge = {}", text_of(&app.ui.badge)));
        }
        if !text_of(&app.ui.detail).starts_with("app2_gainmap.jpg") {
            problems.push(format!("detail after select 0 = {}", text_of(&app.ui.detail)));
        }

        // the HLG files must name their flavour, not just "HDR" (index 1 = ICC cicp)
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        if text_of(&app.ui.badge) != "HLG" {
            problems.push(format!("after select 1 (cicp): badge = {}", text_of(&app.ui.badge)));
        }

        // the MP4 must be read through the sample-entry path, not the HEIF property path
        record_click(&app.state, &app.ui, 6, ClickMods::default());
        if text_of(&app.ui.badge) != "HLG" {
            problems.push(format!("after select 6 (mp4): badge = {}", text_of(&app.ui.badge)));
        }
        if !text_of(&app.ui.detail).starts_with("hlg_video.mp4") {
            problems.push(format!("detail after select 6 = {}", text_of(&app.ui.detail)));
        }
        if !text_of(&app.ui.detail).contains("video") {
            problems.push(format!("detail for the mp4 must say video: {}", text_of(&app.ui.detail)));
        }

        // the mislabeled JPEG must stay SDR even with HDR enabled
        record_click(&app.state, &app.ui, 7, ClickMods::default());
        if text_of(&app.ui.badge) != "SDR" {
            problems.push(format!("after select 7 (mislabeled): badge = {}", text_of(&app.ui.badge)));
        }

        // the APP2 gain map beats the ICC name -> "HDR", not "HLG"
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        if text_of(&app.ui.badge) != "HDR" {
            problems.push(format!("after select 0 (app2 gain map): badge = {}", text_of(&app.ui.badge)));
        }

        // toggle HDR off: the HLG entry must show SDR
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        app.ui.toggle.emit_by_name::<()>("clicked", &[]);
        if text_of(&app.ui.badge) != "SDR" {
            problems.push(format!("after toggle off: badge = {}", text_of(&app.ui.badge)));
        }
        for i in 0..11 {
            let t = row_label_text(&app.ui.list, i);
            if !t.ends_with("SDR") {
                problems.push(format!("row {i} label after toggle off = {t}"));
            }
        }

        // toggle back on: the HLG entry names its flavour again
        record_click(&app.state, &app.ui, 4, ClickMods::default());
        app.ui.toggle.emit_by_name::<()>("clicked", &[]);
        if text_of(&app.ui.badge) != "HLG" {
            problems.push(format!("after toggle on: badge = {}", text_of(&app.ui.badge)));
        }
        let want_after_on = ["HDR", "HLG", "HDR", "HLG", "HLG", "HLG", "HLG", "SDR", "SDR", "SDR", "SDR"];
        for (i, want) in want_after_on.iter().enumerate() {
            let t = row_label_text(&app.ui.list, i as i32);
            if !t.ends_with(want) {
                problems.push(format!("row {i} label after toggle on = {t} (want {want})"));
            }
        }
        // the grid must show the same transfer function as the list for the same file
        {
            let names = [
                "app2_gainmap.jpg",
                "cicp_hlg.jpg",
                "gainmap.jpg",
                "heif_icc_cicp.heic",
                "hlg_icc.jpg",
                "hlg_photo.heic",
                "hlg_video.mp4",
                "mislabeled.jpg",
                "sample.png",
                "sdr_exif.jpg",
                "sdr_photo.heic",
            ];
            let labels = app.ui.labels.borrow();
            for (i, want) in want_after_on.iter().enumerate() {
                let path = fixture_dir().join(names[i]);
                let hits: Vec<&gtk::Label> = labels.iter().filter(|(p, _)| *p == path).map(|(_, l)| l).collect();
                if hits.is_empty() {
                    problems.push(format!("no label recorded for {}", names[i]));
                    continue;
                }
                for l in hits {
                    let t = text_of(l);
                    if !t.ends_with(want) {
                        problems.push(format!("label {} = {t} (want {want})", names[i]));
                    }
                }
            }
        }
        // the info panel must name the signalling source - this is what separates a signalled
        // file from a mislabeled one.
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        if text_of(&app.ui.info) != "gain map 4.0× (ISO 21496-1)" {
            problems.push(format!("info for app2_gainmap.jpg = {:?}", text_of(&app.ui.info)));
        }
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        if text_of(&app.ui.info) != "HLG (ICC cicp)" {
            problems.push(format!("info for cicp_hlg.jpg = {:?}", text_of(&app.ui.info)));
        }
        // a HEIF with no nclx transfer must be signalled by its ICC profile's cicp tag
        record_click(&app.state, &app.ui, 3, ClickMods::default());
        if text_of(&app.ui.info) != "HLG (ICC cicp)" {
            problems.push(format!("info for heif_icc_cicp.heic = {:?}", text_of(&app.ui.info)));
        }
        record_click(&app.state, &app.ui, 7, ClickMods::default());
        if text_of(&app.ui.info) != "sRGB (EXIF)" {
            problems.push(format!("info for mislabeled.jpg = {:?}", text_of(&app.ui.info)));
        }
        record_click(&app.state, &app.ui, 4, ClickMods::default());
        if text_of(&app.ui.info) != "HLG (ICC name)" {
            problems.push(format!("info for hlg_icc.jpg = {:?}", text_of(&app.ui.info)));
        }
        record_click(&app.state, &app.ui, 6, ClickMods::default());
        if text_of(&app.ui.info) != "HLG (nclx) · avc1 · 12.5s" {
            problems.push(format!("info for hlg_video.mp4 = {:?}", text_of(&app.ui.info)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_snapshot_matches_baseline() {
    let problems = gtk4::test_synced(|| {
        let app = App::new(&fixture_dir());
        // The entry must be non-editable AND out of the focus chain BEFORE present(): a focused
        // GtkEntry draws a blinking caret (19 px at x=9, y=46..64) plus a focus ring, so the
        // baseline alternated between caret-on and caret-off frames until both were disabled first.
        freeze_search(&app.ui);
        app.window.present();

        let mut problems = Vec::new();
        // Select the decodable fixture (index 8) so the detail picture gets a paintable through the
        // normal selection path (not by poking the widget directly).
        record_click(&app.state, &app.ui, 8, ClickMods::default());

        // rows are built lazily, so the baseline must be taken after they exist - otherwise the
        // snapshot only shows the sidebar chrome and a new fixture would not change it at all.
        if !settle_rows(&app.ui.list, 11, Duration::from_secs(10)) {
            problems.push("list rows were not built within 10s".to_string());
            return problems;
        }

        // The sidebar list must have real height. With 2 dropdowns, 1 check button, 4 buttons and 2
        // labels stacked in the sidebar, the ScrolledWindow used to be squeezed to ~46 px and only one
        // row was visible - the snapshot then showed no thumbnail at all.
        let viewport = app.ui.list.parent().and_then(|p| p.downcast::<gtk::Viewport>().ok());
        match viewport {
            Some(v) => {
                // allocation only exists after a paint pass, so poll before asserting
                let end = Instant::now() + Duration::from_secs(10);
                while v.height() == 0 && Instant::now() < end {
                    pump(20);
                }
                if v.height() < 200 {
                    problems.push(format!("list viewport height = {}", v.height()));
                }
                // scroll the row with the decodable thumbnail into the visible area
                if let Some(sw) = v.parent().and_then(|p| p.downcast::<gtk::ScrolledWindow>().ok()) {
                    let adj = sw.vadjustment();
                    adj.set_value(adj.upper() - adj.page_size());
                    for _ in 0..40 {
                        pump(20);
                    }
                    // row bounds are in the list's own coordinate space, so visibility is measured
                    // against the scroll offset, not against the viewport's native bounds
                    let rb = app.ui.list.row_at_index(8).and_then(|r| r.bounds());
                    match rb {
                        Some(rb) => {
                            let top = adj.value();
                            let y = rb.1 as f64;
                            if y < top || y + rb.3 as f64 > top + v.height() as f64 {
                                problems.push(format!(
                                    "row 8 not visible: row {rb:?} scroll {top} height {}",
                                    v.height()
                                ));
                            }
                        }
                        None => problems.push("no accessible bounds for row 8".to_string()),
                    }
                } else {
                    problems.push("viewport is not inside a ScrolledWindow".to_string());
                }
            }
            None => problems.push("list is not inside a Viewport".to_string()),
        }

        // Timeline guard: every fixture is created now, so there is one month group plus the
        // "all months" row. Without a minimum content height the sidebar controls squeeze this area
        // to a few pixels and the rows are not rendered.
        let end = Instant::now() + Duration::from_secs(10);
        while app.ui.timeline.height() == 0 && Instant::now() < end {
            pump(20);
        }
        if app.ui.timeline.height() < 100 {
            problems.push(format!("timeline height = {}", app.ui.timeline.height()));
        }
        let tl_rows: Vec<String> = (0..2)
            .filter_map(|i| app.ui.timeline.row_at_index(i))
            .map(|r| plain_row_text(&app.ui.timeline, r.index()))
            .collect();
        if tl_rows.len() != 2 || !tl_rows[0].starts_with("all months") {
            problems.push(format!("timeline rows = {tl_rows:?}"));
        }

        // the picture node only appears after a real paint pass
        app.window.queue_draw();
        for _ in 0..60 {
            pump(20);
        }

        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("content snapshot produced no node within 10s".to_string());
            return problems;
        }

        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("gtk_detail.png");
        let baseline = base.join("gtk_detail.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }
        if red_pixels(&out, 240) == 0 {
            problems.push("snapshot has no rendered thumbnail in the sidebar".to_string());
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_glycin_applies_exif_orientation() {
    let size = gtk4::test_synced(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oriented.jpg");
        let tex = spectrum_gtk::app::decode_texture(&path).expect("glycin should decode the JPEG fixture");
        (tex.width(), tex.height())
    });
    // 8x6 source with Orientation=6 (rotate 90 CW) -> 6x8 after transformation
    assert_eq!(size, (6, 8));
}

#[test]
fn e2e_gtk_edit_ops_and_sidecar_roundtrip() {
    let dir = fixture_dir();
    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();
        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // index 8 = sample.png, the only fixture glycin can decode (8x6)
        record_click(&app.state, &app.ui, 8, ClickMods::default());
        let sample = dir.join("sample.png");
        let frame = match spectrum_gtk::app::decode_frame(&sample) {
            Ok(f) => f,
            Err(e) => {
                problems.push(format!("decode: {e}"));
                return problems;
            }
        };
        if (frame.width(), frame.height()) != (8, 6) {
            problems.push(format!("source size = {}x{}", frame.width(), frame.height()));
            return problems;
        }

        // rotate 90 through the button: 8x6 -> 6x8
        app.ui.rotate.emit_by_name::<()>("clicked", &[]);
        let edit = spectrum_gtk::app::edit_of(&app.state.borrow());
        if edit.display_size(8, 6) != (6, 8) {
            problems.push(format!("display_size after rotate = {:?}", edit.display_size(8, 6)));
        }
        if text_of(&app.ui.detail) != "sample.png  ·  photo  ·  6x8" {
            problems.push(format!("detail after rotate = {}", text_of(&app.ui.detail)));
        }
        match spectrum_gtk::edit::apply_edit(&frame, &edit) {
            Ok(tex) if (tex.width() as u32, tex.height() as u32) == (6, 8) => {}
            Ok(tex) => problems.push(format!("rotated texture = {}x{}", tex.width(), tex.height())),
            Err(e) => problems.push(format!("apply_edit: {e}")),
        }

        // flip does not change the size, but it does change the transform
        app.ui.flip.emit_by_name::<()>("clicked", &[]);
        let edit = spectrum_gtk::app::edit_of(&app.state.borrow());
        if edit.display_size(8, 6) != (6, 8) || !edit.flip_h || edit.rotation != 270 {
            problems.push(format!("edit after flip = {edit:?}"));
        }

        // crop is stored in the rotated space: half of 6x8 -> 3x4
        {
            let st = &mut *app.state.borrow_mut();
            spectrum_gtk::app::apply_op(st, &app.ui, EditOp::Crop(CropRect::new(0.25, 0.25, 0.5, 0.5)));
        }
        let edit = spectrum_gtk::app::edit_of(&app.state.borrow());
        if edit.display_size(8, 6) != (3, 4) {
            problems.push(format!("display_size after crop = {:?}", edit.display_size(8, 6)));
        }
        if text_of(&app.ui.detail) != "sample.png  ·  photo  ·  3x4" {
            problems.push(format!("detail after crop = {}", text_of(&app.ui.detail)));
        }

        // sidecar write + read must round-trip
        let st = app.state.borrow();
        match spectrum_gtk::app::save_sidecar(&st) {
            Ok(path) => {
                if !path.exists() {
                    problems.push("sidecar not written".to_string());
                }
                let back = spectrum_core::xmp::read_sidecar(&sample, 1);
                let want = spectrum_core::xmp::SidecarData {
                    rotation: edit.rotation,
                    flip_h: edit.flip_h,
                    crop: edit.crop,
                    gyro_config: None,
                };
                if back != Some(want) {
                    problems.push(format!("round-trip = {back:?} but edit = {edit:?}"));
                }
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => problems.push(format!("save_sidecar: {e}")),
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_sidecar_edit_applied_on_scan() {
    let dir = PathBuf::from("/tmp/spectrum-sidecar-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let sample = dir.join("sample.png");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.png"),
        &sample,
    )
    .unwrap();

    // Rotate 90 stored in the sidecar: an 8x6 file must display as 6x8 after a fresh scan.
    let stored = spectrum_core::edit::CompositeEdit::from(&[EditOp::Rotate(90)]);
    spectrum_core::xmp::write_sidecar(&sample, &stored, 1, None).unwrap();

    // a second file so the selection can actually change (selected-notify only fires on change)
    fixture::write_png_fixture(&dir, "a_other.png", 8, 6, [0, 0, 0]);

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();

        let entries = spectrum_core::scan::scan_folder(&dir);
        if entries.len() != 2 {
            problems.push(format!("scan entries = {}", entries.len()));
            return problems;
        }
        match entries[1].edit.as_ref() {
            Some(sd) if sd.rotation == 90 && !sd.flip_h => {}
            Some(sd) => problems.push(format!("scanned sidecar edit = {sd:?}")),
            None => problems.push("scanned entry has no sidecar edit".to_string()),
        }
        if entries[0].edit.is_some() {
            problems.push("a_other.png should have no sidecar".to_string());
        }

        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        if text_of(&app.ui.detail) != "sample.png  ·  photo  ·  6x8 (sidecar)" {
            problems.push(format!("detail = {}", text_of(&app.ui.detail)));
        }
        problems
    });

    spectrum_core::xmp::delete_sidecar(&sample);
    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_thumbnail_reflects_edit() {
    let dir = fixture_dir();
    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();
        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let thumbs = |name: &str| -> Vec<gdk::Texture> {
            app.ui
                .thumbs_pics
                .borrow()
                .iter()
                .filter(|(p, _)| p.file_name().map(|f| f == name).unwrap_or(false))
                .filter_map(|(_, pic)| pic.paintable().and_then(|p| p.downcast::<gdk::Texture>().ok()))
                .collect()
        };

        let before = thumbs("sample.png");
        if before.is_empty() {
            problems.push("no thumbnail texture for sample.png".to_string());
            return problems;
        }
        for tex in &before {
            if (tex.width() as u32, tex.height() as u32) != (8, 6) {
                problems.push(format!("thumbnail before edit = {}x{}", tex.width(), tex.height()));
            }
        }

        let decodes_before = app.ui.thumbs.borrow().decodes();
        app.ui.rotate.emit_by_name::<()>("clicked", &[]);

        let after = thumbs("sample.png");
        if after.len() != before.len() {
            problems.push(format!("thumbnail count changed: {} -> {}", before.len(), after.len()));
        }
        for tex in &after {
            if (tex.width() as u32, tex.height() as u32) != (6, 8) {
                problems.push(format!("thumbnail after rotate = {}x{}", tex.width(), tex.height()));
            }
        }

        // repainting must reuse the cached frame, not decode again
        if app.ui.thumbs.borrow().decodes() != decodes_before {
            problems.push(format!(
                "decodes {} -> {} (thumbnail cache must be reused)",
                decodes_before,
                app.ui.thumbs.borrow().decodes()
            ));
        }
        problems
    });
    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_crop_applied_to_display() {
    let dir = PathBuf::from("/tmp/spectrum-crop-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let wide = fixture::write_png_fixture(&dir, "wide.png", 100, 50, [255, 0, 0]);

    // Crop is stored in the rotated space: 100x50 -> rotate 90 -> 50x100, right half -> 25x100.
    let stored = spectrum_core::edit::CompositeEdit::from(&[
        EditOp::Rotate(90),
        EditOp::Crop(CropRect::new(0.5, 0.0, 0.5, 1.0)),
    ]);
    spectrum_core::xmp::write_sidecar(&wide, &stored, 1, None).unwrap();

    // a second file so the selection can actually change (selected-notify only fires on change)
    fixture::write_png_fixture(&dir, "a_other.png", 8, 6, [0, 0, 0]);

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();

        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // sorted by path: a_other.png(0), wide.png(1)
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        if text_of(&app.ui.detail) != "wide.png  ·  photo  ·  25x100 (sidecar)" {
            problems.push(format!("detail = {}", text_of(&app.ui.detail)));
        }

        let thumbs: Vec<gdk::Texture> = app
            .ui
            .thumbs_pics
            .borrow()
            .iter()
            .filter(|(p, _)| p.file_name().map(|f| f == "wide.png").unwrap_or(false))
            .filter_map(|(_, pic)| pic.paintable().and_then(|p| p.downcast::<gdk::Texture>().ok()))
            .collect();
        if thumbs.is_empty() {
            problems.push("no thumbnail texture for wide.png".to_string());
            return problems;
        }
        for tex in &thumbs {
            if (tex.width() as u32, tex.height() as u32) != (25, 100) {
                problems.push(format!("crop texture = {}x{} (expected 25x100)", tex.width(), tex.height()));
            }
        }
        problems
    });

    spectrum_core::xmp::delete_sidecar(&wide);
    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_exif_metadata_panel() {
    let dir = PathBuf::from("/tmp/spectrum-exif-fixtures");
    std::fs::create_dir_all(&dir).unwrap();

    let f = fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        custom_rendered: None,
        make: Some("Sony"),
        model: Some("ILCE-7RM2"),
        iso: Some(100),
        exposure: Some((1, 250)),
        f_number: Some((28, 10)),
        focal_length: Some((55, 1)),
        gain_map: false,
        cicp: None,
        dmnd: None,
    date_time_original: None,
};
    fixture::write_jpeg_fixture_exif(&dir, "camera.jpg", None, &f);
    // a second file so the selection can actually change (selected-notify only fires on change)
    fixture::write_png_fixture(&dir, "a_other.png", 8, 6, [0, 0, 0]);

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();

        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // sorted by name: a_other.png (0), camera.jpg (1)
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        let want = "sRGB (EXIF) · Sony ILCE-7RM2 · 1/250s · f/2.8 · 55mm · ISO 100";
        if text_of(&app.ui.info) != want {
            problems.push(format!("info for camera.jpg = {:?}", text_of(&app.ui.info)));
        }

        // a file with no EXIF must show an empty panel, not stale text from the previous file
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        if text_of(&app.ui.info) != "" {
            problems.push(format!("info for a_other.png = {:?}", text_of(&app.ui.info)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_real_fixtures_badge() {
    // Real encoder-produced files (copied from the macOS reference), not synthetic writers.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spectrum-core/tests/fixtures");
    if !dir.exists() {
        eprintln!("skip: {} not found", dir.display());
        return;
    }

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();

        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let model = app.ui.selection.model().ok_or("no model".to_string()).unwrap();
        if model.n_items() != 6 {
            problems.push(format!("model items = {}", model.n_items()));
            return problems;
        }

        // sorted by name: 0 hlg_mislabeled_srgb.heic, 1 hlg_synthetic_patches.heic, 2 hlg_video.mp4,
        // 3 sdr_photo.jpg, 4 sdr_video.mp4, 5 slog3_mislabeled_srgb.heic
        let want = ["SDR", "HLG", "HLG", "SDR", "SDR", "SDR"];
        for (i, badge) in want.iter().enumerate() {
            record_click(&app.state, &app.ui, i, ClickMods::default());
            if text_of(&app.ui.badge) != *badge {
                problems.push(format!("real fixture {i}: badge = {}", text_of(&app.ui.badge)));
            }
            let label = row_label_text(&app.ui.list, i as i32);
            if !label.ends_with(format!("  ·  {badge}").as_str()) {
                problems.push(format!("real fixture {i}: row label = {label}"));
            }
        }

        // the FFmpeg-written MP4 must be recognised as a video, and the info panel must show the
        // codec + duration read from stsd/mvhd (plain-box layout, no version/flags).
        record_click(&app.state, &app.ui, 2, ClickMods::default());
        if !text_of(&app.ui.detail).contains("video") {
            problems.push(format!("detail for hlg_video.mp4 = {}", text_of(&app.ui.detail)));
        }
        if text_of(&app.ui.info) != "HLG (nclx) · hvc1 · 0.3s" {
            problems.push(format!("info for hlg_video.mp4 = {:?}", text_of(&app.ui.info)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_thumbnail_cache_is_bounded() {
    let dir = PathBuf::from("/tmp/spectrum-lru-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let a = fixture::write_png_fixture(&dir, "a.png", 100, 100, [255, 0, 0]);
    let b = fixture::write_png_fixture(&dir, "b.png", 100, 100, [0, 255, 0]);
    let c = fixture::write_png_fixture(&dir, "c.png", 100, 100, [0, 0, 255]);

    let problems = gtk4::test_synced(move || {
        let app = App::new(&dir);
        let mut problems = Vec::new();

        // The app must use a bounded cache, not an unbounded one.
        if app.ui.thumbs.borrow().capacity() != 256 {
            problems.push(format!("app cache capacity = {}", app.ui.thumbs.borrow().capacity()));
        }

        // Start from an empty cache so the sequence below is deterministic (App::new already
        // painted every thumbnail through the same service).
        let base;
        {
            let mut svc = app.ui.thumbs.borrow_mut();
            svc.clear();
            svc.set_capacity(2);
            base = (svc.hits(), svc.decodes(), svc.evictions());
        }
        let pic = gtk::Picture::new();
        let paint = |path: &Path| {
            let state = app.state.borrow();
            spectrum_gtk::app::paint_thumbnail(&app.ui, &state, path, &pic);
        };
        let counters = || {
            let svc = app.ui.thumbs.borrow();
            (
                svc.hits() - base.0,
                svc.decodes() - base.1,
                svc.evictions() - base.2,
                svc.len(),
            )
        };

        paint(&a);
        paint(&b);
        if counters() != (0, 2, 0, 2) {
            problems.push(format!("after a+b: {:?}", counters()));
        }

        // A hit on `a` makes `b` the oldest, so the next insert must evict `b`, not `a`.
        paint(&a);
        if counters() != (1, 2, 0, 2) {
            problems.push(format!("after hit on a: {:?}", counters()));
        }
        paint(&c);
        if counters() != (1, 3, 1, 2) {
            problems.push(format!("after c: {:?}", counters()));
        }
        let cached = (
            app.ui.thumbs.borrow().is_cached(&a),
            app.ui.thumbs.borrow().is_cached(&b),
            app.ui.thumbs.borrow().is_cached(&c),
        );
        if cached != (true, false, true) {
            problems.push(format!("cached a/b/c = {cached:?} (expected a+c cached, b evicted)"));
        }

        // `b` was evicted, so painting it again must decode again.
        paint(&b);
        if counters() != (1, 4, 2, 2) {
            problems.push(format!("after re-painting b: {:?}", counters()));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_sort_changes_row_order() {
    let dir = PathBuf::from("/tmp/spectrum-sort-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    fixture::write_png_fixture(&dir, "a.png", 8, 6, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "b.png", 8, 6, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "c.png", 8, 6, [0, 0, 0]);
    // mtimes deliberately not in name order, so Date sort differs from Name sort
    for (name, secs) in [("a.png", 100u64), ("b.png", 300), ("c.png", 200)] {
        let f = std::fs::OpenOptions::new().write(true).open(dir.join(name)).unwrap();
        f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();

        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // Rows are rebuilt when the model changes, which happens on the next main-loop pass, so
        // pump until the order settles instead of asserting immediately.
        let settle = |list: &gtk::ListBox, want: &[String]| -> Vec<String> {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let got: Vec<String> = (0..want.len()).map(|i| row_label_text(list, i as i32)).collect();
                if got == want || Instant::now() >= end {
                    return got;
                }
                pump(20);
            }
        };
        let model_paths = |store: &gio::ListStore| -> Vec<String> {
            let model = store.clone().upcast::<gio::ListModel>();
            (0..3)
                .map(|i| model.item(i))
                .flatten()
                .map(|o| {
                    o.downcast::<glib::BoxedAnyObject>()
                        .unwrap()
                        .borrow::<spectrum_core::MediaEntry>()
                        .name()
                })
                .collect()
        };

        let want_name: Vec<String> = vec!["a.png  ·  SDR".to_string(), "b.png  ·  SDR".to_string(), "c.png  ·  SDR".to_string()];
        if settle(&app.ui.list, &want_name) != want_name {
            problems.push(format!("name order = {:?}", row_names(&app.ui.list)));
        }

        // select b, then switch to Date sort: the selection must follow the same file
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        app.ui.sort_box.set_selected(1); // Date
        let want_date: Vec<String> = vec!["b.png  ·  SDR".to_string(), "c.png  ·  SDR".to_string(), "a.png  ·  SDR".to_string()];
        if settle(&app.ui.list, &want_date) != want_date {
            problems.push(format!("date order = {:?}", row_names(&app.ui.list)));
        }
        if model_paths(&app.ui.store) != vec!["b.png", "c.png", "a.png"] {
            problems.push(format!("model order after Date sort = {:?}", model_paths(&app.ui.store)));
        }
        if !app.ui.selection.is_selected(0) || app.ui.selection.selection().size() != 1 {
            problems.push(format!("selection after sort = {:?}", selected_indices(&app.ui)));
        }
        if !text_of(&app.ui.detail).starts_with("b.png") {
            problems.push(format!("detail after sort = {:?}", text_of(&app.ui.detail)));
        }

        // back to Name
        app.ui.sort_box.set_selected(0);
        if settle(&app.ui.list, &want_name) != want_name {
            problems.push(format!("name order after switching back = {:?}", row_names(&app.ui.list)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_filter_by_kind_and_hdr() {
    let dir = PathBuf::from("/tmp/spectrum-filter-fixtures");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    fixture::write_png_fixture(&dir, "a_photo.png", 8, 6, [0, 0, 0]);
    fixture::write_jpeg_fixture(&dir, "b_hdr.jpg", Some("Rec. ITU-R BT.2100 HLG"), Some(1), None, 1);
    fixture::write_fixture(&dir, "c_video.mp4", 9, 18, 0, true);
    fixture::write_png_fixture(&dir, "d_photo.png", 8, 6, [0, 0, 0]);

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();
        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let settle = |list: &gtk::ListBox, want: &[String]| -> Vec<String> {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let got: Vec<String> = (0..want.len()).map(|i| row_label_text(list, i as i32)).collect();
                if got == want || Instant::now() >= end {
                    return got;
                }
                pump(20);
            }
        };

        let all = vec![
            "a_photo.png  ·  SDR".to_string(),
            "b_hdr.jpg  ·  HLG".to_string(),
            "c_video.mp4  ·  HLG".to_string(),
            "d_photo.png  ·  SDR".to_string(),
        ];
        if settle(&app.ui.list, &all) != all {
            problems.push(format!("unfiltered order = {:?}", row_names(&app.ui.list)));
        }

        // Select the HDR JPEG, then filter to photos: it stays visible and stays selected.
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        app.ui.filter_kind.set_selected(1); // Photos
        let photos = vec![
            "a_photo.png  ·  SDR".to_string(),
            "b_hdr.jpg  ·  HLG".to_string(),
            "d_photo.png  ·  SDR".to_string(),
        ];
        if settle(&app.ui.list, &photos) != photos {
            problems.push(format!("photos order = {:?}", row_names(&app.ui.list)));
        }
        if !text_of(&app.ui.detail).starts_with("b_hdr.jpg") {
            problems.push(format!("detail after kind filter = {:?}", text_of(&app.ui.detail)));
        }

        // HDR only: only the signalled photo remains.
        app.ui.filter_hdr_only.set_active(true);
        let hdr_photos = vec!["b_hdr.jpg  ·  HLG".to_string()];
        if settle(&app.ui.list, &hdr_photos) != hdr_photos {
            problems.push(format!("hdr-only photos = {:?}", row_names(&app.ui.list)));
        }

        // Videos + HDR only: the selection is no longer visible, so it must be cleared.
        app.ui.filter_hdr_only.set_active(false);
        app.ui.filter_kind.set_selected(2); // Videos
        let videos = vec!["c_video.mp4  ·  HLG".to_string()];
        if settle(&app.ui.list, &videos) != videos {
            problems.push(format!("videos = {:?}", row_names(&app.ui.list)));
        }
        // The selected file is no longer visible, so the selection falls back to the first visible
        // row instead of pointing at a filtered-out entry.
        if app.state.borrow().selected != Some(0) {
            problems.push(format!(
                "selection after filtering out = {:?}",
                app.state.borrow().selected
            ));
        }
        if !text_of(&app.ui.detail).starts_with("c_video.mp4") {
            problems.push(format!("detail after filtering out = {:?}", text_of(&app.ui.detail)));
        }

        // Clearing the filter restores all four rows.
        app.ui.filter_kind.set_selected(0);
        if settle(&app.ui.list, &all) != all {
            problems.push(format!("after clearing filter = {:?}", row_names(&app.ui.list)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_transfer_filter_and_status() {
    let dir = PathBuf::from("/tmp/spectrum-transfer-fixtures");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    fixture::write_jpeg_fixture(&dir, "a_gainmap.jpg", None, Some(1), Some(3), 1);
    fixture::write_jpeg_fixture(&dir, "b_hlg.jpg", Some("Rec. ITU-R BT.2100 HLG"), Some(1), None, 1);
    fixture::write_jpeg_fixture(&dir, "c_pq.jpg", Some("Rec. ITU-R BT.2100 PQ"), Some(1), None, 1);
    fixture::write_png_fixture(&dir, "d_plain.png", 8, 6, [0, 0, 0]);

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();
        let app = App::new(&dir);
        app.window.present();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let settle = |list: &gtk::ListBox, want: &[String]| -> Vec<String> {
            let end = Instant::now() + Duration::from_secs(5);
            loop {
                let got: Vec<String> = (0..want.len()).map(|i| row_label_text(list, i as i32)).collect();
                if got == want || Instant::now() >= end {
                    return got;
                }
                pump(20);
            }
        };

        let all = vec![
            "a_gainmap.jpg  ·  HDR".to_string(),
            "b_hlg.jpg  ·  HLG".to_string(),
            "c_pq.jpg  ·  PQ".to_string(),
            "d_plain.png  ·  SDR".to_string(),
        ];
        if settle(&app.ui.list, &all) != all {
            problems.push(format!("unfiltered order = {:?}", row_names(&app.ui.list)));
        }
        if text_of(&app.ui.status) != "4 of 4 · all kinds · sorted by Name (natural)" {
            problems.push(format!("status = {:?}", text_of(&app.ui.status)));
        }

        // HLG only: the gain-map file is HDR but has no transfer code, so it must drop out.
        app.ui.transfer_box.set_selected(1);
        let hlg_only = vec!["b_hlg.jpg  ·  HLG".to_string()];
        if settle(&app.ui.list, &hlg_only) != hlg_only {
            problems.push(format!("HLG only = {:?}", row_names(&app.ui.list)));
        }
        if text_of(&app.ui.status) != "1 of 4 · all kinds · HLG · sorted by Name (natural)" {
            problems.push(format!("status after HLG filter = {:?}", text_of(&app.ui.status)));
        }

        // HDR only (no transfer restriction) brings the gain-map file back: 3 of 4.
        app.ui.transfer_box.set_selected(0);
        app.ui.filter_hdr_only.set_active(true);
        let hdr_only = vec![
            "a_gainmap.jpg  ·  HDR".to_string(),
            "b_hlg.jpg  ·  HLG".to_string(),
            "c_pq.jpg  ·  PQ".to_string(),
        ];
        if settle(&app.ui.list, &hdr_only) != hdr_only {
            problems.push(format!("HDR only = {:?}", row_names(&app.ui.list)));
        }
        if text_of(&app.ui.status) != "3 of 4 · all kinds · HDR only · sorted by Name (natural)" {
            problems.push(format!("status after HDR filter = {:?}", text_of(&app.ui.status)));
        }

        // Sort by Kind + photos only: status must reflect both.
        app.ui.filter_hdr_only.set_active(false);
        app.ui.filter_kind.set_selected(1);
        app.ui.sort_box.set_selected(2);
        if text_of(&app.ui.status) != "4 of 4 · photo · sorted by Kind (natural)" {
            problems.push(format!("status with kind filter + kind sort = {:?}", text_of(&app.ui.status)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_keyboard_navigation() {
    let dir = PathBuf::from("/tmp/spectrum-nav-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"] {
        fixture::write_png_fixture(&dir, name, 8, 6, [10, 20, 30]);
    }

    let problems = gtk4::test_synced(move || {
        let mut problems = Vec::new();
        let app = App::new(&dir);
        pump(20);

        // The navigation helper assumes min_columns, so the test must pin that.
        if app.ui.grid.min_columns() != 3 {
            problems.push(format!("grid min_columns = {}", app.ui.grid.min_columns()));
        }

        record_click(&app.state, &app.ui, 0, ClickMods::default());
        let moves = [
            (Direction::Down, 3, "d.png"),
            (Direction::Right, 4, "e.png"),
            (Direction::Up, 1, "b.png"),
            (Direction::Home, 0, "a.png"),
            (Direction::End, 5, "f.png"),
        ];
        for (dir, want_pos, want_name) in moves {
            let next = spectrum_gtk::app::move_selection(&app.state, &app.ui, dir);
            if next != Some(want_pos) {
                problems.push(format!("{:?} -> {:?}", dir, next));
                continue;
            }
            if app.state.borrow().selected != Some(want_pos) {
                problems.push(format!("{:?}: state.selected = {:?}", dir, app.state.borrow().selected));
            }
            if !text_of(&app.ui.detail).starts_with(want_name) {
                problems.push(format!("{:?}: detail = {}", dir, text_of(&app.ui.detail)));
            }
        }

        // Moving past the last item must not wrap: selection stays on f.png.
        let next = spectrum_gtk::app::move_selection(&app.state, &app.ui, Direction::Down);
        if next.is_some() {
            problems.push(format!("Down past the end returned {:?}", next));
        }
        if app.state.borrow().selected != Some(5) {
            problems.push(format!("after wrap attempt: selected = {:?}", app.state.borrow().selected));
        }
        problems
    });
    assert!(problems.is_empty(), "e2e failures: {:?}", problems);
}

#[test]
fn e2e_gtk_subfolder_cover_and_navigation() {
    let root = PathBuf::from("/tmp/spectrum-subfolder-fixtures");
    std::fs::create_dir_all(&root).unwrap();
    for sub in ["a-folder", "b-folder"] {
        let d = root.join(sub);
        std::fs::create_dir_all(&d).unwrap();
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_file() {
                std::fs::remove_file(&p).unwrap();
            }
        }
    }
    // image must win over the media file even though the media name sorts first
    fixture::write_png_fixture(&root.join("a-folder"), "z.png", 8, 6, [255, 0, 0]);
    fixture::write_mp4_fixture(&root.join("a-folder"), "a.mp4", 9, 16, 0, true);

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let labels: Vec<String> = (0..2)
            .filter_map(|i| app.ui.subfolders.row_at_index(i))
            .filter_map(|r| r.child().and_then(|c| c.downcast::<gtk::Label>().ok()))
            .map(|l| text_of(&l))
            .collect();
        if labels != vec!["a-folder  ·  z.png", "b-folder  ·  (no cover)"] {
            problems.push(format!("subfolder rows = {labels:?}"));
        }

        let row0 = match app.ui.subfolders.row_at_index(0) {
            Some(r) => r,
            None => {
                problems.push("no subfolder row 0".to_string());
                return problems;
            }
        };
        app.ui.subfolders.emit_by_name::<()>("row-activated", &[&row0]);
        if !settle_rows(&app.ui.list, 1, Duration::from_secs(10)) {
            problems.push("a-folder file list never built".to_string());
            return problems;
        }
        if app.state.borrow().folder != root2.join("a-folder") {
            problems.push(format!("folder after click = {:?}", app.state.borrow().folder));
        }
        if app.state.borrow().entries.len() != 2 {
            problems.push(format!("a-folder entries = {}", app.state.borrow().entries.len()));
        }
        if !text_of(&app.ui.status).starts_with("2 of 2") {
            problems.push(format!("status after entering a-folder = {:?}", text_of(&app.ui.status)));
        }

        // a-folder has no subfolders, so the list must now be empty for the current folder
        if app.ui.subfolders.row_at_index(0).is_some() {
            problems.push("subfolder list should be empty inside a-folder".to_string());
        }
        // navigate back to a sibling folder through the same entry point
        open_folder(&app.state, &app.ui, &root2.join("b-folder"));
        for _ in 0..20 {
            pump(20);
        }
        if app.state.borrow().entries.len() != 0 {
            problems.push(format!("b-folder entries = {}", app.state.borrow().entries.len()));
        }
        if text_of(&app.ui.detail) != "(no selection)" {
            problems.push(format!("detail in empty folder = {:?}", text_of(&app.ui.detail)));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_search_results_and_navigation() {
    let problems = gtk4::test_synced(move || {
        let root = Path::new("/tmp/spectrum-search-ui-fixtures").to_path_buf();
        std::fs::create_dir_all(&root).unwrap();
        fixture::write_fixture(&root, "hlg_photo.png", 9, 18, 0, true);
        fixture::write_fixture(&root, "hlg_notes.txt", 1, 13, 0, true); // not media
        fixture::write_fixture(&root, ".hlg_hidden.png", 1, 13, 0, true); // hidden
        let sub = root.join("hlg-sub");
        std::fs::create_dir_all(&sub).unwrap();
        fixture::write_fixture(&sub, "hlg_nested.png", 9, 18, 0, true);

        let app = App::new(&root);
        app.window.present();
        settle_snapshot(&app.window, Duration::from_millis(3000));

        let mut problems = Vec::new();

        // Too short: nothing is searched, the subfolder area stays visible.
        app.ui.search.set_text("h");
        for _ in 0..20 {
            pump(20);
        }
        if app.ui.results_scroll.is_visible() {
            problems.push("results area visible for a 1-char query".to_string());
        }
        if !app.ui.sub_scroll.is_visible() {
            problems.push("subfolder area hidden for a 1-char query".to_string());
        }

        app.ui.search.set_text("hlg");
        for _ in 0..20 {
            pump(20);
        }
        if !app.ui.results_scroll.is_visible() {
            problems.push("results area not visible while searching".to_string());
        }
        if app.ui.sub_scroll.is_visible() {
            problems.push("subfolder area still visible while searching".to_string());
        }

        let labels: Vec<String> = (0..8)
            .filter_map(|i| {
                app.ui
                    .results
                    .row_at_index(i)
                    .map(|_| plain_row_text(&app.ui.results, i))
            })
            .collect();
        if labels.len() != 3 {
            problems.push(format!("expected 3 result rows, got {labels:?}"));
            return problems;
        }
        if labels[0] != "hlg-sub  ·  folder" {
            problems.push(format!("first result row should be the folder match, got {labels:?}"));
        }
        let mut photos = labels[1..].to_vec();
        photos.sort();
        if photos != vec!["hlg_nested.png  ·  photo".to_string(), "hlg_photo.png  ·  photo".to_string()] {
            problems.push(format!("photo rows wrong: {labels:?}"));
        }

        // Click a photo result: opens its folder and selects that file.
        let photo_row = app.ui.results.row_at_index(2).unwrap();
        app.ui.results.emit_by_name::<()>("row-activated", &[&photo_row]);
        for _ in 0..20 {
            pump(20);
        }
        if app.state.borrow().folder != root {
            problems.push(format!("folder should stay at root, got {:?}", app.state.borrow().folder));
        }
        if !text_of(&app.ui.detail).starts_with("hlg_photo.png") {
            problems.push(format!("detail should show the clicked file, got {}", text_of(&app.ui.detail)));
        }

        // Click the folder result: navigates into it.
        let folder_row = app.ui.results.row_at_index(0).unwrap();
        app.ui.results.emit_by_name::<()>("row-activated", &[&folder_row]);
        for _ in 0..20 {
            pump(20);
        }
        if app.state.borrow().folder != sub {
            problems.push(format!("folder should be hlg-sub, got {:?}", app.state.borrow().folder));
        }
        if app.state.borrow().entries.len() != 1 {
            problems.push(format!("hlg-sub should have 1 entry, got {}", app.state.borrow().entries.len()));
        }
        problems
    });
    assert!(problems.is_empty(), "{:?}", problems);
}

fn set_mtime(dir: &Path, name: &str, secs: u64) {
    let t = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    std::fs::File::options()
        .write(true)
        .open(dir.join(name))
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn e2e_gtk_timeline_groups_and_month_filter() {
    let root = PathBuf::from("/tmp/spectrum-timeline-fixtures");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    fixture::write_png_fixture(&root, "a_oct.png", 8, 6, [255, 0, 0]);
    fixture::write_png_fixture(&root, "b_jun.png", 8, 6, [255, 0, 0]);
    fixture::write_png_fixture(&root, "c_jun.png", 8, 6, [255, 0, 0]);
    // Fixed mtimes so the month groups are reproducible regardless of when the test runs.
    set_mtime(&root, "a_oct.png", 1791158400); // 2026-10-05 UTC
    set_mtime(&root, "b_jun.png", 1780272000); // 2026-06-01 UTC
    set_mtime(&root, "c_jun.png", 1780358400); // 2026-06-02 UTC

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let texts: Vec<String> = (0..3)
            .filter_map(|i| app.ui.timeline.row_at_index(i))
            .map(|r| plain_row_text(&app.ui.timeline, r.index()))
            .collect();
        if texts != vec!["all months  ·  3", "2026-10  ·  1", "2026-06  ·  2"] {
            problems.push(format!("timeline rows = {texts:?}"));
        }

        let row1 = match app.ui.timeline.row_at_index(1) {
            Some(r) => r,
            None => {
                problems.push("no timeline row 1".to_string());
                return problems;
            }
        };
        app.ui.timeline.emit_by_name::<()>("row-activated", &[&row1]);
        for _ in 0..20 {
            pump(20);
        }
        let status = text_of(&app.ui.status);
        if !status.contains("1 of 3") || !status.contains("2026-10") {
            problems.push(format!("status after month filter = {status}"));
        }
        if app.state.borrow().display != vec![0] {
            problems.push(format!("display after month filter = {:?}", app.state.borrow().display));
        }

        // Row 0 clears the month filter.
        let row0 = match app.ui.timeline.row_at_index(0) {
            Some(r) => r,
            None => {
                problems.push("no timeline row 0".to_string());
                return problems;
            }
        };
        app.ui.timeline.emit_by_name::<()>("row-activated", &[&row0]);
        for _ in 0..20 {
            pump(20);
        }
        let status = text_of(&app.ui.status);
        if !status.contains("3 of 3") || status.contains("2026-10") {
            problems.push(format!("status after clearing the month = {status}"));
        }
        problems
    });
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn e2e_gtk_multi_select_with_modifiers() {
    let root = PathBuf::from("/tmp/spectrum-multi-fixtures");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a.png", "b.png", "c.png"] {
        fixture::write_png_fixture(&root, name, 8, 6, [255, 0, 0]);
    }

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // GTK has no in-process key injection, so the click goes through the same helper the key
        // controller uses; `record_click` mirrors the core set into both views.
        let sel = |i: u32| {
            record_click(&app.state, &app.ui, i as usize, *app.ui.mods.borrow());
            for _ in 0..10 {
                pump(20);
            }
        };

        // the highlight must match the core model in both the grid model and the list rows
        let check = |problems: &mut Vec<String>, what: &str, expected: &[usize]| {
            let grid: Vec<usize> = (0..3).filter(|&i| app.ui.selection.is_selected(i as u32)).collect();
            let list: Vec<usize> = app.ui.list.selected_rows().iter().map(|r| r.index() as usize).collect();
            if grid != expected {
                problems.push(format!("{} grid highlight = {:?}", what, grid));
            }
            if list != expected {
                problems.push(format!("{} list highlight = {:?}", what, list));
            }
        };

        // the startup selection is recorded even though it was set programmatically
        if app.ui.multi.borrow().set != vec![0] {
            problems.push(format!("startup selection = {:?}", app.ui.multi.borrow().set));
        }
        check(&mut problems, "startup selection", &[0]);

        // plain click replaces the selection
        sel(1);
        if app.ui.multi.borrow().set != vec![1] {
            problems.push(format!("after plain click = {:?}", app.ui.multi.borrow().set));
        }
        check(&mut problems, "after plain click", &[1]);

        // Ctrl-click adds
        app.ui.mods.borrow_mut().ctrl = true;
        sel(2);
        if app.ui.multi.borrow().set != vec![1, 2] {
            problems.push(format!("after ctrl-click = {:?}", app.ui.multi.borrow().set));
        }
        check(&mut problems, "after ctrl-click", &[1, 2]);
        if !text_of(&app.ui.status).contains("selected 2") {
            problems.push(format!("status after ctrl-click = {}", text_of(&app.ui.status)));
        }

        // Shift extends from the anchor (the Ctrl-click left it at 2), so 0..2 inclusive
        app.ui.mods.borrow_mut().ctrl = false;
        app.ui.mods.borrow_mut().shift = true;
        sel(0);
        if app.ui.multi.borrow().set != vec![0, 1, 2] {
            problems.push(format!("after shift-click = {:?}", app.ui.multi.borrow().set));
        }
        check(&mut problems, "after shift-click", &[0, 1, 2]);

        // Ctrl-click on a selected row toggles it off
        app.ui.mods.borrow_mut().shift = false;
        app.ui.mods.borrow_mut().ctrl = true;
        sel(1);
        if app.ui.multi.borrow().set != vec![0, 2] {
            problems.push(format!("after toggle-off = {:?}", app.ui.multi.borrow().set));
        }
        check(&mut problems, "after toggle-off", &[0, 2]);

        // a plain click resets to one row
        app.ui.mods.borrow_mut().ctrl = false;
        sel(2);
        if app.ui.multi.borrow().set != vec![2] {
            problems.push(format!("after reset = {:?}", app.ui.multi.borrow().set));
        }
        check(&mut problems, "after reset", &[2]);
        if text_of(&app.ui.status).contains("selected") {
            problems.push(format!("status should not show a count for one row: {}", text_of(&app.ui.status)));
        }
        problems
    });
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn e2e_gtk_snapshot_multi_select() {
    let root = PathBuf::from("/tmp/spectrum-multi-snapshot");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a.png", "b.png", "c.png"] {
        fixture::write_png_fixture(&root, name, 8, 6, [255, 0, 0]);
    }

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        record_click(&app.state, &app.ui, 0, ClickMods::default());
        for _ in 0..10 {
            pump(20);
        }
        record_click(&app.state, &app.ui, 2, ClickMods { ctrl: true, shift: false });
        for _ in 0..10 {
            pump(20);
        }
        if !text_of(&app.ui.status).contains("selected 2") {
            problems.push(format!("status = {}", text_of(&app.ui.status)));
        }
        // the highlight must show both rows, in the grid model and in the list rows
        let grid: Vec<usize> = (0..3).filter(|&i| app.ui.selection.is_selected(i as u32)).collect();
        let list: Vec<usize> = app.ui.list.selected_rows().iter().map(|r| r.index() as usize).collect();
        if grid != vec![0, 2] {
            problems.push(format!("grid highlight = {:?}", grid));
        }
        if list != vec![0, 2] {
            problems.push(format!("list highlight = {:?}", list));
        }

        let out = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/multi_select.baseline.png");
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("no paint pass before snapshot".to_string());
            return problems;
        }
        if !settle_picture(&app.ui.picture, &app.window) {
            problems.push("picture/window size not settled before snapshot".to_string());
            return problems;
        }

        let new = out.with_extension("new.png");
        // The detail picture is filled by glycin asynchronously, so the snapshot is only stable
        // once the picture node is actually rendered. Without this the test passed alone (more
        // iterations before the snapshot) but failed when other tests ran first.
        let mut rendered = false;
        for _ in 0..10 {
            if let Err(e) = snapshot_png(&app.window, &new) {
                problems.push(format!("snapshot failed: {e}"));
                return problems;
            }
            if red_pixels_total(&new) > 100_000 {
                rendered = true;
                break;
            }
            for _ in 0..20 {
                pump(20);
            }
        }
        if !rendered {
            problems.push(format!("detail picture never rendered (red px = {})", red_pixels_total(&new)));
            return problems;
        }
        if out.exists() {
            let a = std::fs::read(&new).map_err(|e| e.to_string()).unwrap();
            let b = std::fs::read(&out).map_err(|e| e.to_string()).unwrap();
            if a != b {
                problems.push(format!(
                    "snapshot differs from baseline ({} vs {} bytes); {}; detail={} info={} picture={}x{} window={}x{}",
                    a.len(),
                    b.len(),
                    diff_bbox(&new, &out),
                    text_of(&app.ui.detail),
                    text_of(&app.ui.info),
                    app.ui.picture.width(),
                    app.ui.picture.height(),
                    app.window.width(),
                    app.window.height()
                ));
            }
        } else {
            std::fs::rename(&new, &out).map_err(|e| e.to_string()).unwrap();
            eprintln!("wrote baseline snapshot: {}", out.display());
        }
        problems
    });
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn e2e_gtk_delete_selected_rows_to_trash() {
    let root = PathBuf::from("/tmp/spectrum-delete-fixtures");
    let trash = PathBuf::from("/tmp/spectrum-delete-trash");
    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&trash).ok();
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a.png", "b.png"] {
        fixture::write_png_fixture(&root, name, 8, 6, [255, 0, 0]);
    }

    // The app reads SPECTRUM_TRASH_ROOT, so the test never touches the user's real trash.
    std::env::set_var("SPECTRUM_TRASH_ROOT", &trash);
    let root2 = root.clone();
    let trash2 = trash.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        record_click(&app.state, &app.ui, 0, ClickMods::default());
        for _ in 0..10 {
            pump(20);
        }
        app.ui.mods.borrow_mut().ctrl = true;
        record_click(&app.state, &app.ui, 1, ClickMods { ctrl: true, shift: false });
        for _ in 0..10 {
            pump(20);
        }
        if app.ui.multi.borrow().set != vec![0, 1] {
            problems.push(format!("selection before delete = {:?}", app.ui.multi.borrow().set));
        }

        app.ui.delete.emit_by_name::<()>("clicked", &[]);
        for _ in 0..20 {
            pump(20);
        }

        if !text_of(&app.ui.status).contains("deleted: 2 to trash") {
            problems.push(format!("status after delete = {}", text_of(&app.ui.status)));
        }
        // rows are rebuilt when the model changes; poll before asserting so the check does not
        // depend on how many iterations the signal needs
        let end = Instant::now() + Duration::from_secs(5);
        while list_row_count(&app.ui.list) != 0 && Instant::now() < end {
            pump(20);
        }
        if list_row_count(&app.ui.list) != 0 {
            problems.push(format!("list after delete rows={}", list_row_count(&app.ui.list)));
        }
        if app.ui.multi.borrow().count() != 0 {
            problems.push(format!("multi after delete = {:?}", app.ui.multi.borrow().set));
        }

        let mut stored: Vec<String> = std::fs::read_dir(trash2.join("files"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        stored.sort();
        if stored != vec!["a.png", "b.png"] {
            problems.push(format!("trash files = {stored:?}"));
        }
        let mut infos: Vec<String> = std::fs::read_dir(trash2.join("info"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        infos.sort();
        if infos != vec!["a.png.trashinfo", "b.png.trashinfo"] {
            problems.push(format!("trash info = {infos:?}"));
        }
        let one = std::fs::read_to_string(trash2.join("info/a.png.trashinfo")).unwrap();
        if !one.contains("[Trash Info]") || !one.contains(&root2.join("a.png").display().to_string()) {
            problems.push(format!("trashinfo content = {one}"));
        }
        problems
    });
    std::env::remove_var("SPECTRUM_TRASH_ROOT");
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn e2e_gtk_clipboard_copy_cut_paste_and_rename() {
    let root = PathBuf::from("/tmp/spectrum-clipboard-e2e");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    fixture::write_png_fixture(&root, "a.png", 8, 6, [255, 0, 0]);

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let click = |b: &gtk::Button| {
            b.emit_by_name::<()>("clicked", &[]);
            for _ in 0..20 {
                pump(20);
            }
        };

        record_click(&app.state, &app.ui, 0, ClickMods::default());
        for _ in 0..10 {
            pump(20);
        }

        // Copy then paste into the same folder: the collision rule adds the " 1" counter.
        click(&app.ui.copy);
        if app.ui.clipboard.borrow().mode != ClipboardMode::Copy {
            problems.push(format!("clipboard mode after Copy = {:?}", app.ui.clipboard.borrow().mode));
        }
        click(&app.ui.paste);
        if !text_of(&app.ui.status).contains("pasted copied: 1 ok") {
            problems.push(format!("status after copy paste = {}", text_of(&app.ui.status)));
        }
        if !root2.join("a.png 1").exists() {
            problems.push("copy paste did not create 'a.png 1'".to_string());
        }

        // Cut moves the file into the folder that is currently open.
        click(&app.ui.cut);
        if app.ui.clipboard.borrow().mode != ClipboardMode::Cut {
            problems.push(format!("clipboard mode after Cut = {:?}", app.ui.clipboard.borrow().mode));
        }
        let sub = root2.join("sub");
        open_folder(&app.state, &app.ui, &sub);
        for _ in 0..20 {
            pump(20);
        }
        click(&app.ui.paste);
        if !text_of(&app.ui.status).contains("pasted moved: 1 ok") {
            problems.push(format!("status after cut paste = {}", text_of(&app.ui.status)));
        }
        if !sub.join("a.png").exists() {
            problems.push("cut paste did not move the file into sub/".to_string());
        }
        if root2.join("a.png").exists() {
            problems.push("cut left the source file behind".to_string());
        }

        // Rename the folder that is open (sub -> sub-renamed), then the app follows the new path.
        app.ui.rename_entry.set_text("sub-renamed");
        click(&app.ui.rename);
        let renamed = root2.join("sub-renamed");
        if !renamed.exists() {
            problems.push(format!("rename did not create {}", renamed.display()));
        }
        if app.state.borrow().folder != renamed {
            problems.push(format!("folder after rename = {}", app.state.borrow().folder.display()));
        }

        // An invalid name is reported in the status bar instead of moving the folder.
        app.ui.rename_entry.set_text("bad/name");
        click(&app.ui.rename);
        if !text_of(&app.ui.status).contains("rename failed: invalid name") {
            problems.push(format!("status after bad rename = {}", text_of(&app.ui.status)));
        }
        problems
    });
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn e2e_gtk_sort_by_size_and_direction() {
    let root = PathBuf::from("/tmp/spectrum-sort-size-fixtures");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    // box-only files: the size is what the sort uses, and fixed byte counts keep the order stable
    for (name, n) in [("a.png", 100usize), ("b.png", 300usize), ("c.png", 200usize)] {
        std::fs::write(root.join(name), vec![0u8; n]).unwrap();
    }

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let pump20 = |n: u32| {
            for _ in 0..n {
                pump(20);
            }
        };

        if app.state.borrow().display != vec![0, 1, 2] {
            problems.push(format!("initial display = {:?}", app.state.borrow().display));
        }

        // Size is index 4 in the sort dropdown
        app.ui.sort_box.set_selected(4);
        pump20(20);
        if app.state.borrow().display != vec![1, 2, 0] {
            problems.push(format!("display sorted by size = {:?}", app.state.borrow().display));
        }
        if !text_of(&app.ui.status).contains("sorted by Size (natural)") {
            problems.push(format!("status = {}", text_of(&app.ui.status)));
        }

        app.ui.reverse.emit_by_name::<()>("clicked", &[]);
        pump20(20);
        if app.state.borrow().display != vec![0, 2, 1] {
            problems.push(format!("display reversed = {:?}", app.state.borrow().display));
        }
        if !text_of(&app.ui.status).contains("sorted by Size (reversed)") {
            problems.push(format!("status after reverse = {}", text_of(&app.ui.status)));
        }

        // the direction is remembered when the sort key changes
        app.ui.sort_box.set_selected(0);
        pump20(20);
        if app.state.borrow().display != vec![2, 1, 0] {
            problems.push(format!("display after switching to Name (reversed) = {:?}", app.state.borrow().display));
        }
        problems
    });
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn e2e_gtk_subfolders_sorted_by_cover_date() {
    let root = PathBuf::from("/tmp/spectrum-subfolder-sort-fixtures");
    std::fs::create_dir_all(&root).unwrap();
    for sub in ["z-newest", "m-middle", "a-oldest", "b-tie", "c-empty"] {
        let d = root.join(sub);
        std::fs::create_dir_all(&d).unwrap();
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_file() {
                std::fs::remove_file(&p).unwrap();
            }
        }
    }
    for sub in ["z-newest", "m-middle", "a-oldest", "b-tie"] {
        fixture::write_png_fixture(&root.join(sub), "cover.png", 8, 6, [255, 0, 0]);
    }
    set_mtime(&root.join("z-newest"), "cover.png", 1791158400); // 2026-10-05
    set_mtime(&root.join("m-middle"), "cover.png", 1785484800); // 2026-07-31
    set_mtime(&root.join("a-oldest"), "cover.png", 1780272000); // 2026-06-01
    set_mtime(&root.join("b-tie"), "cover.png", 1780272000); // same date as a-oldest

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let want = vec![
            "z-newest  ·  cover.png".to_string(),
            "m-middle  ·  cover.png".to_string(),
            "a-oldest  ·  cover.png".to_string(),
            "b-tie  ·  cover.png".to_string(),
            "c-empty  ·  (no cover)".to_string(),
        ];
        let got: Vec<String> = (0..5).map(|i| plain_row_text(&app.ui.subfolders, i)).collect();
        if got != want {
            problems.push(format!("subfolder order by cover date = {got:?}"));
        }

        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("subfolder_sort.png");
        let baseline = base.join("subfolder_sort.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }

        // The path list must follow the same order as the rows, otherwise activating a row opens the
        // wrong folder.
        let paths: Vec<String> = app
            .ui
            .subfolder_paths
            .borrow()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        if paths != vec!["z-newest", "m-middle", "a-oldest", "b-tie", "c-empty"] {
            problems.push(format!("subfolder paths = {paths:?}"));
        }

        // Activating the first row must open the folder shown at that position (the newest cover), not
        // the alphabetically first one.
        let Some(row) = app.ui.subfolders.row_at_index(0) else {
            problems.push("no subfolder row 0".to_string());
            return problems;
        };
        app.ui.subfolders.emit_by_name::<()>("row-activated", &[&row]);
        for _ in 0..20 {
            pump(20);
        }
        let folder = app.state.borrow().folder.clone();
        if folder != root2.join("z-newest") {
            problems.push(format!("activating row 0 -> {folder:?}, expected z-newest"));
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_folder_list_cache_is_reused_until_invalidate() {
    let root = PathBuf::from("/tmp/spectrum-folder-cache-e2e");
    // Remove the whole tree: a leftover subfolder from a previous run would show up in the cached
    // listing and make the order assertions depend on earlier runs.
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for sub in ["a", "b"] {
        std::fs::create_dir_all(&root.join(sub)).unwrap();
    }
    fixture::write_png_fixture(&root.join("a"), "cover.png", 8, 6, [255, 0, 0]);
    fixture::write_png_fixture(&root.join("b"), "cover.png", 8, 6, [255, 0, 0]);
    set_mtime(&root.join("a"), "cover.png", 1780272000); // 2026-06-01
    set_mtime(&root.join("b"), "cover.png", 1791158400); // 2026-10-05

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let want = vec!["b  ·  cover.png".to_string(), "a  ·  cover.png".to_string()];
        let got: Vec<String> = (0..2).map(|i| plain_row_text(&app.ui.subfolders, i)).collect();
        if got != want {
            problems.push(format!("initial subfolder rows = {got:?}"));
        }
        if !app.ui.cache.borrow().is_scanned(&root2) {
            problems.push("sidebar build did not use the folder cache".to_string());
        }

        // A directory created after the scan must not appear: the cached listing is reused.
        std::fs::create_dir_all(root2.join("c")).unwrap();
        fixture::write_png_fixture(&root2.join("c"), "cover.png", 8, 6, [0, 0, 255]);
        set_mtime(&root2.join("c"), "cover.png", 1785484800); // 2026-07-31, between b and a
        for _ in 0..10 {
            pump(20);
        }
        if (0..2).map(|i| plain_row_text(&app.ui.subfolders, i)).collect::<Vec<String>>() != want {
            problems.push("cached listing was re-read for a new directory".to_string());
        }

        // Invalidating (what a folder monitor event does) forces the rescan.
        app.ui.cache.borrow_mut().invalidate(&root2);
        rebuild_subfolders(&app.state, &app.ui);
        for _ in 0..10 {
            pump(20);
        }
        let after: Vec<String> = (0..3).map(|i| plain_row_text(&app.ui.subfolders, i)).collect();
        if after != vec![
            "b  ·  cover.png".to_string(),
            "c  ·  cover.png".to_string(),
            "a  ·  cover.png".to_string(),
        ] {
            problems.push(format!("rows after invalidate = {after:?}"));
        }

        // Snapshot needs a paint pass; the earlier settle proves the window is mapped, but re-settle
        // here so the snapshot is taken after the invalidate/rebuild.
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window not snapshot-able after invalidate".to_string());
            return problems;
        }
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("folder_cache.png");
        let baseline = base.join("folder_cache.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_undo_redo_edit_ops() {
    let root = PathBuf::from("/tmp/spectrum-undo-fixtures");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a.png", "b.png", "c.png"] {
        fixture::write_png_fixture(&root, name, 8, 6, [255, 0, 0]);
    }
    let root2 = root.clone();

    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let click = |button: &gtk::Button| {
            button.emit_by_name::<()>("clicked", &[]);
            for _ in 0..10 {
                pump(20);
            }
        };

        record_click(&app.state, &app.ui, 0, ClickMods::default());
        pump(20);

        app.ui.rotate.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if app.state.borrow().ops != vec![EditOp::Rotate(90)] {
            problems.push(format!("ops after rotate = {:?}", app.state.borrow().ops));
        }
        if !text_of(&app.ui.status).contains("history 1 undo, 0 redo") {
            problems.push(format!("status after rotate = {}", text_of(&app.ui.status)));
        }

        app.ui.flip.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if app.state.borrow().ops != vec![EditOp::Rotate(90), EditOp::FlipH] {
            problems.push(format!("ops after flip = {:?}", app.state.borrow().ops));
        }
        if !text_of(&app.ui.status).contains("history 2 undo, 0 redo") {
            problems.push(format!("status after flip = {}", text_of(&app.ui.status)));
        }

        // undo restores the ops from before the flip
        app.ui.undo.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if app.state.borrow().ops != vec![EditOp::Rotate(90)] {
            problems.push(format!("ops after undo = {:?}", app.state.borrow().ops));
        }
        if !text_of(&app.ui.status).contains("history 1 undo, 1 redo") {
            problems.push(format!("status after undo = {}", text_of(&app.ui.status)));
        }

        app.ui.redo.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if app.state.borrow().ops != vec![EditOp::Rotate(90), EditOp::FlipH] {
            problems.push(format!("ops after redo = {:?}", app.state.borrow().ops));
        }

        // undoing both changes empties the ops and leaves the whole branch in redo
        app.ui.undo.emit_by_name::<()>("clicked", &[]);
        app.ui.undo.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if !app.state.borrow().ops.is_empty() {
            problems.push(format!("ops after two undos = {:?}", app.state.borrow().ops));
        }
        if !text_of(&app.ui.status).contains("history 0 undo, 2 redo") {
            problems.push(format!("status after two undos = {}", text_of(&app.ui.status)));
        }

        // a new edit drops the undone branch
        app.ui.flip.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if app.state.borrow().ops != vec![EditOp::FlipH] {
            problems.push(format!("ops after new edit = {:?}", app.state.borrow().ops));
        }
        if !text_of(&app.ui.status).contains("history 1 undo, 0 redo") {
            problems.push(format!("status after new edit = {}", text_of(&app.ui.status)));
        }

        // The change is tied to a file, so undo must follow that file even when another one is
        // selected. Edit b.png, select c.png, then undo.
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        for _ in 0..10 {
            pump(20);
        }
        app.ui.rotate.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        record_click(&app.state, &app.ui, 2, ClickMods::default());
        for _ in 0..10 {
            pump(20);
        }
        app.ui.undo.emit_by_name::<()>("clicked", &[]);
        for _ in 0..10 {
            pump(20);
        }
        if app.state.borrow().selected != Some(1) {
            problems.push(format!(
                "undo did not follow the file the change belongs to: selected = {:?}",
                app.state.borrow().selected
            ));
        }
        if app.state.borrow().ops != vec![EditOp::FlipH] {
            problems.push(format!("ops after cross-file undo = {:?}", app.state.borrow().ops));
        }

        if !settle_picture(&app.ui.picture, &app.window) {
            problems.push("picture/window size not settled before snapshot".to_string());
            return problems;
        }

        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("no paint pass before snapshot".to_string());
            return problems;
        }
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("undo_redo.png");
        let baseline = base.join("undo_redo.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_status_bar_progress_and_expiry() {
    let root = PathBuf::from("/tmp/spectrum-status-bar-fixtures");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    for name in ["a.png", "b.png", "c.png"] {
        fixture::write_png_fixture(&root, name, 8, 6, [255, 0, 0]);
    }
    for name in ["d.png", "e.png"] {
        fixture::write_png_fixture(&root.join("sub"), name, 8, 6, [0, 255, 0]);
    }
    let trash = PathBuf::from("/tmp/spectrum-status-bar-trash");
    std::fs::remove_dir_all(&trash).ok();
    std::env::set_var("SPECTRUM_TRASH_ROOT", &trash);

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();

        // The completion TTL is a deadline, so the test drives an explicit clock instead of
        // sleeping 10 seconds.
        let clock = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        *app.ui.clock.borrow_mut() = clock;

        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // Nothing has run yet, so the progress row is hidden.
        if app.ui.progress.is_visible() {
            problems.push(format!(
                "progress row visible at startup: {}",
                text_of(&app.ui.progress)
            ));
        }

        // Scanning a subfolder produces a completion message.
        open_folder(&app.state, &app.ui, &root2.join("sub"));
        pump(20);
        if !app.ui.progress.is_visible() {
            problems.push("progress row hidden after a scan".to_string());
        }
        if text_of(&app.ui.progress) != "scanned 2" {
            problems.push(format!("progress after scan = {}", text_of(&app.ui.progress)));
        }

        // Still visible 9 s later, gone at exactly 10 s.
        *app.ui.clock.borrow_mut() = clock + Duration::from_secs(9);
        refresh(&app.state.borrow(), &app.ui);
        pump(20);
        if text_of(&app.ui.progress) != "scanned 2" {
            problems.push(format!("progress at 9 s = {}", text_of(&app.ui.progress)));
        }

        *app.ui.clock.borrow_mut() = clock + Duration::from_secs(10);
        refresh(&app.state.borrow(), &app.ui);
        pump(20);
        if app.ui.progress.is_visible() {
            problems.push(format!(
                "completion message did not expire: {}",
                text_of(&app.ui.progress)
            ));
        }

        // A determinate task renders the count.
        app.ui.bar.borrow_mut().begin_total("pasting", 3);
        app.ui.bar.borrow_mut().update(1, "pasting d.png");
        refresh(&app.state.borrow(), &app.ui);
        pump(20);
        if text_of(&app.ui.progress) != "pasting d.png 1/3" {
            problems.push(format!("progress during task = {}", text_of(&app.ui.progress)));
        }

        // A real handler must report its own progress: copy one file, paste it, then delete it.
        record_click(&app.state, &app.ui, 0, ClickMods::default());
        pump(20);
        app.ui.copy.emit_by_name::<()>("clicked", &[]);
        pump(20);
        app.ui.paste.emit_by_name::<()>("clicked", &[]);
        pump(20);
        if text_of(&app.ui.progress) != "pasted 1" {
            problems.push(format!("progress after paste = {}", text_of(&app.ui.progress)));
        }

        // A plain click resets the selection: `apply_view` already recorded a click for the first
        // visible row after the paste, so a Ctrl-click here would have added to it.
        record_click(&app.state, &app.ui, 1, ClickMods::default());
        pump(20);
        if app.ui.multi.borrow().count() != 1 {
            problems.push(format!("selection before delete = {:?}", app.ui.multi.borrow().set));
        }
        app.ui.delete.emit_by_name::<()>("clicked", &[]);
        pump(20);
        if text_of(&app.ui.progress) != "deleted 1" {
            problems.push(format!("progress after delete = {}", text_of(&app.ui.progress)));
        }
        if !text_of(&app.ui.status).contains("deleted: 1 to trash") {
            problems.push(format!("status after delete = {}", text_of(&app.ui.status)));
        }

        // A new task replaces the pending completion message instead of waiting for its TTL.
        app.ui.bar.borrow_mut().begin("scanning");
        refresh(&app.state.borrow(), &app.ui);
        pump(20);
        if text_of(&app.ui.progress) != "scanning" {
            problems.push(format!("progress after new task = {}", text_of(&app.ui.progress)));
        }
        *app.ui.clock.borrow_mut() = clock + Duration::from_secs(60);
        refresh(&app.state.borrow(), &app.ui);
        pump(20);
        if text_of(&app.ui.progress) != "scanning" {
            problems.push(format!(
                "the cancelled timer cleared the new task: {}",
                text_of(&app.ui.progress)
            ));
        }

        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("no paint pass before snapshot".to_string());
            return problems;
        }
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("status_bar.png");
        let baseline = base.join("status_bar.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_import_panel_date_groups_and_expand() {
    let root = PathBuf::from("/tmp/spectrum-import-e2e");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();
    // Fixed mtimes so the day groups are deterministic:
    // 1700000000 -> 2023-11-14, 1691000000 -> 2023-08-02, 1684400000 -> 2023-05-18.
    let days: [(&str, u64); 4] = [
        ("a.png", 1_700_000_000),
        ("b.png", 1_691_000_000),
        ("c.png", 1_691_000_000),
        ("d.png", 1_684_400_000),
    ];
    for (name, secs) in days {
        fixture::write_png_fixture(&root, name, 8, 6, [255, 0, 0]);
        std::fs::File::options()
            .write(true)
            .open(root.join(name))
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        // Rows must exist after the first paint.
        let mut waited = 0;
        while app.ui.import_list.height() == 0 && waited < 100 {
            pump(20);
            waited += 1;
        }
        if app.ui.import_list.height() == 0 {
            problems.push("import list never allocated".to_string());
            return problems;
        }

        let want = vec![
            "2023-11-14  ·  1",
            "a.png",
            "2023-08-02  ·  2",
            "b.png",
            "c.png",
            "2023-05-18  ·  1",
            "d.png",
        ];
        let got: Vec<String> = (0..list_row_count(&app.ui.import_list))
            .map(|i| plain_row_text(&app.ui.import_list, i))
            .collect();
        if got != want {
            problems.push(format!("import rows = {got:?}, want {want:?}"));
        }

        // Activating a group header collapses just that group.
        if let Some(row) = app.ui.import_list.row_at_index(0) {
            app.ui.import_list.emit_by_name::<()>("row-activated", &[&row]);
            pump(20);
        }
        let after: Vec<String> = (0..list_row_count(&app.ui.import_list))
            .map(|i| plain_row_text(&app.ui.import_list, i))
            .collect();
        if after != vec![
            "2023-11-14  ·  1",
            "2023-08-02  ·  2",
            "b.png",
            "c.png",
            "2023-05-18  ·  1",
            "d.png",
        ] {
            problems.push(format!("rows after collapsing group 0 = {after:?}"));
        }

        // collapse all -> headers only
        app.ui.import_collapse.emit_by_name::<()>("clicked", &[]);
        pump(20);
        if list_row_count(&app.ui.import_list) != 3 {
            problems.push(format!(
                "rows after collapse all = {}",
                list_row_count(&app.ui.import_list)
            ));
        }

        // expand all -> back to 7
        app.ui.import_expand.emit_by_name::<()>("clicked", &[]);
        pump(20);
        if list_row_count(&app.ui.import_list) != 7 {
            problems.push(format!(
                "rows after expand all = {}",
                list_row_count(&app.ui.import_list)
            ));
        }

        // Activating an item row selects that file in the main list.
        if let Some(row) = app.ui.import_list.row_at_index(3) {
            app.ui.import_list.emit_by_name::<()>("row-activated", &[&row]);
            pump(20);
        }
        if app.state.borrow().selected != Some(1) {
            problems.push(format!(
                "activating b.png did not select it: selected = {:?}",
                app.state.borrow().selected
            ));
        }

        // A rescan keeps the expansion state per group key.
        app.ui.import_model.borrow_mut().toggle(1);
        rescan_import(&app.state, &app.ui);
        pump(20);
        if list_row_count(&app.ui.import_list) != 5 {
            problems.push(format!(
                "rows after rescan with group 1 collapsed = {}",
                list_row_count(&app.ui.import_list)
            ));
        }

        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("no paint pass before snapshot".to_string());
            return problems;
        }
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("import_panel.png");
        let baseline = base.join("import_panel.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}

#[test]
fn e2e_gtk_import_panel_uses_exif_date() {
    let root = PathBuf::from("/tmp/spectrum-exif-date-e2e");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();

    // photo.jpg: EXIF date 2020-01-02 wins over mtime 2023-11-14.
    let f = fixture::ExifFixture {
        orientation: 1,
        date_time_original: Some("2020:01:02 03:04:05"),
        ..fixture::ExifFixture::default()
    };
    fixture::write_jpeg_fixture_exif(&root, "photo.jpg", None, &f);
    set_mtime(&root, "photo.jpg", 1_700_000_000);

    // plain.png has no EXIF, so it falls back to mtime (2021-06-05).
    fixture::write_png_fixture(&root, "plain.png", 8, 6, [255, 0, 0]);
    set_mtime(&root, "plain.png", 1_622_851_200);

    let root2 = root.clone();
    let problems = gtk4::test_synced(move || {
        let app = App::new(&root2);
        freeze_search(&app.ui);
        app.window.present();
        let mut problems = Vec::new();
        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("window never mapped/allocated (is gtk4-broadwayd running?)".to_string());
            return problems;
        }

        let mut waited = 0;
        while app.ui.import_list.height() == 0 && waited < 100 {
            pump(20);
            waited += 1;
        }
        if app.ui.import_list.height() == 0 {
            problems.push("import list never allocated".to_string());
            return problems;
        }

        let want = vec![
            "2021-06-05  ·  1",
            "plain.png",
            "2020-01-02  ·  1",
            "photo.jpg",
        ];
        let got: Vec<String> = (0..list_row_count(&app.ui.import_list))
            .map(|i| plain_row_text(&app.ui.import_list, i))
            .collect();
        if got != want {
            problems.push(format!("import rows = {got:?}, want {want:?}"));
        }

        if !settle_snapshot(&app.window, Duration::from_secs(10)) {
            problems.push("no paint pass before snapshot".to_string());
            return problems;
        }
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::create_dir_all(&base).unwrap();
        let out = base.join("import_exif_date.png");
        let baseline = base.join("import_exif_date.baseline.png");
        match snapshot_png(&app.window, &out) {
            Ok(()) => {
                if baseline.exists() {
                    let got = std::fs::read(&out).unwrap();
                    let want = std::fs::read(&baseline).unwrap();
                    if got != want {
                        problems.push(format!(
                            "snapshot differs from baseline ({} vs {} bytes); {}",
                            got.len(),
                            want.len(),
                            diff_bbox(&out, &baseline)
                        ));
                    }
                } else {
                    std::fs::copy(&out, &baseline).unwrap();
                    eprintln!("wrote baseline snapshot: {}", baseline.display());
                }
            }
            Err(e) => problems.push(format!("snapshot: {e}")),
        }
        problems
    });

    assert!(problems.is_empty(), "e2e failures: {problems:?}");
}
