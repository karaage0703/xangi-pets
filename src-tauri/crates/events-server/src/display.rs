pub const PET_SCALE_STEPS: [f64; 6] = [1.0, 1.5, 2.0, 2.5, 3.0, 4.0];

pub fn valid_scale(scale: f64) -> bool {
    PET_SCALE_STEPS.contains(&scale)
}

pub fn next_scale(scale: f64) -> f64 {
    let index = PET_SCALE_STEPS
        .iter()
        .position(|value| *value == scale)
        .unwrap_or(0);
    PET_SCALE_STEPS[(index + 1) % PET_SCALE_STEPS.len()]
}

/// Same 1x sprite dimensions as the permanent pet, plus room for its label.
pub fn session_geometry(scale: f64) -> (u32, u32, u32, u32) {
    let width = (96.0 * scale).round() as u32;
    let height = (104.0 * scale).round() as u32;
    (width, height, (width + 20).max(200), height + 70)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scales_cycle_without_accepting_invalid_or_non_finite_values() {
        assert_eq!(next_scale(4.0), 1.0);
        for invalid in [0.0, 6.0, f64::NAN, f64::INFINITY] {
            assert!(!valid_scale(invalid));
        }
        assert!(valid_scale(1.5));
    }
    #[test]
    fn all_sizes_fit_their_windows_and_match_the_permanent_sprite() {
        assert_eq!(session_geometry(1.0), (96, 104, 200, 174));
        assert_eq!(session_geometry(4.0), (384, 416, 404, 486));
        for scale in PET_SCALE_STEPS {
            let (w, h, ww, wh) = session_geometry(scale);
            assert!(ww >= w + 20 && wh >= h + 60);
        }
    }
}
