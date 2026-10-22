/// Full D4 group: 8 states. Convention: flip first, then rotate.
pub fn exif_orientation_to_transform(ori: i32) -> (i32, bool) {
    match ori {
        1 => (0, false),
        2 => (0, true),
        3 => (180, false),
        4 => (180, true),
        5 => (90, true),
        6 => (90, false),
        7 => (270, true),
        8 => (270, false),
        _ => (0, false),
    }
}

pub fn transform_to_exif_orientation(rotation: i32, flip_h: bool) -> i32 {
    let r = rotation.rem_euclid(360);
    match (r, flip_h) {
        (0, false) => 1,
        (0, true) => 2,
        (180, false) => 3,
        (180, true) => 4,
        (90, true) => 5,
        (90, false) => 6,
        (270, true) => 7,
        (270, false) => 8,
        _ => 1,
    }
}
