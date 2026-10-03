//! Orientation helpers: Source Euler angles and the direction an entity "points" in.

use crate::formats::fgd::Fgd;
use crate::formats::vmf::{parse_vec3, Entity};
use glam::{DMat3, DVec3};

pub fn angles_matrix(a: DVec3) -> DMat3 {
    // Source: yaw about Z, pitch about Y (positive = down), roll about X.
    let (p, y, r) = (a.x.to_radians(), a.y.to_radians(), a.z.to_radians());
    DMat3::from_rotation_z(y) * DMat3::from_rotation_y(p) * DMat3::from_rotation_x(r)
}

pub fn matrix_angles(m: DMat3) -> DVec3 {
    let yaw = m.x_axis.y.atan2(m.x_axis.x);
    let pitch = (-m.x_axis.z).clamp(-1.0, 1.0).asin();
    let roll = m.y_axis.z.atan2(m.z_axis.z);
    let r = |v: f64| {
        let d = v.to_degrees();
        let d = (d * 1000.0).round() / 1000.0;
        if d.abs() < 1e-6 { 0.0 } else { d }
    };
    DVec3::new(r(pitch), r(yaw), r(roll))
}

/// Keys that carry an orientation, in order of preference.
pub const DIRECTION_KEYS: [&str; 3] = ["movedir", "angles", "angle"];

/// Direction for a legacy single-number `angle` value: -1 = up, -2 = down, else yaw.
pub fn yaw_direction(yaw: f64) -> DVec3 {
    if yaw == -1.0 {
        DVec3::Z
    } else if yaw == -2.0 {
        DVec3::NEG_Z
    } else {
        angles_matrix(DVec3::new(0.0, yaw, 0.0)) * DVec3::X
    }
}

/// Direction of a `movedir`/`angles`/`angle` value (3 numbers = pitch yaw roll, 1 number = yaw).
pub fn value_direction(v: &str) -> Option<DVec3> {
    if let Some(a) = parse_vec3(v) {
        return Some(angles_matrix(a) * DVec3::X);
    }
    v.trim().parse::<f64>().ok().map(yaw_direction)
}

/// The direction an entity points in (door move direction, facing, push direction …), if its
/// class or its keys define one.
pub fn entity_direction(e: &Entity, fgd: &Fgd) -> Option<DVec3> {
    let class = fgd.get(e.classname());
    for key in DIRECTION_KEYS {
        let value = e.get(key).map(|s| s.to_string()).or_else(|| {
            class?.props.iter().find(|p| p.name.eq_ignore_ascii_case(key)).map(|p| p.default.clone())
        });
        if let Some(d) = value.and_then(|v| value_direction(&v)) {
            return Some(d);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions() {
        assert!((value_direction("0 90 0").unwrap() - DVec3::Y).length() < 1e-9);
        assert!((value_direction("-90 0 0").unwrap() - DVec3::Z).length() < 1e-9);
        assert_eq!(value_direction("-1"), Some(DVec3::Z));
        assert_eq!(value_direction("-2"), Some(DVec3::NEG_Z));
        assert!(value_direction("abc").is_none());
    }

    #[test]
    fn matrix_roundtrip() {
        let a = DVec3::new(20.0, 130.0, -40.0);
        let b = matrix_angles(angles_matrix(a));
        assert!((a - b).length() < 0.01);
    }
}
