//! The components' shapes, drawn as meshes whose vertex alphas follow a cross-section: crisp, with
//! a one-pixel anti-aliased edge, or as the soft glow of a lit shape.
//!
//! A glow is the cross-section of the shape blurred by a Gaussian, worked out analytically and
//! written into the vertex alphas, so the GPU draws it with no texture and no blur pass. It's in
//! points like everything else, so it scales with the editor and keeps its proportions at any
//! scale.

use std::f32::consts::{PI, SQRT_2, TAU};

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Pos2, Vec2, vec2};

/// A glow under a lit shape, in the signal colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    /// The blur's standard deviation, in points. It sets how far the glow reaches: three sigmas
    /// hold all but a trace of it.
    pub sigma: f32,
    /// How far the shape grows before it's blurred, in points.
    pub spread: f32,
    /// The glow's opacity at full strength, from 0 to 1.
    pub opacity: f32,
}

/// How many sigmas past the shape's edge a glow's mesh reaches.
const REACH_IN_SIGMAS: f32 = 3.0;

/// How many steps a glow's cross-section is sampled in, from its centre to its reach.
const GLOW_STEPS: u16 = 24;

/// One point of a cross-section: the shape's alpha at a distance from its centre.
///
/// The distance is from the centre line of a stroke, or the centre of a disk. A cross-section's
/// stops start at distance 0 and rise; between them the alpha is interpolated linearly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    /// The distance from the centre, in points.
    pub distance: f32,
    /// The alpha there, from 0 to 1.
    pub alpha: f32,
}

/// The cross-section of a solid shape `half_width` points from its centre to its edge, its edge
/// anti-aliased over `feather` points: one physical pixel, `1 / pixels_per_point`.
#[must_use]
pub fn crisp(half_width: f32, feather: f32) -> Vec<Stop> {
    let half_feather = feather / 2.0;
    vec![
        Stop {
            distance: 0.0,
            alpha: 1.0,
        },
        Stop {
            distance: (half_width - half_feather).max(0.0),
            alpha: 1.0,
        },
        Stop {
            distance: half_width + half_feather,
            alpha: 0.0,
        },
    ]
}

impl Glow {
    /// The cross-section of the glow of a shape `half_width` points from its centre to its edge,
    /// at full opacity: the shape, grown by the spread, blurred by the Gaussian. Across a stroke,
    /// that's the difference of two error functions.
    ///
    /// The same formula serves for a disk, where it's a close approximation.
    #[must_use]
    pub fn profile(self, half_width: f32) -> Vec<Stop> {
        let half_width = half_width + self.spread;
        let scale = self.sigma * SQRT_2;
        let reach = self.sigma.mul_add(REACH_IN_SIGMAS, half_width);
        (0..=GLOW_STEPS)
            .map(|step| {
                let distance = reach * f32::from(step) / f32::from(GLOW_STEPS);
                Stop {
                    distance,
                    alpha: 0.5
                        * (erf((distance + half_width) / scale)
                            - erf((distance - half_width) / scale)),
                }
            })
            .collect()
    }
}

/// The error function, to within 1.5e-7 (Abramowitz and Stegun, 7.1.26).
fn erf(x: f32) -> f32 {
    const P: f32 = 0.327_591_1;
    const A: [f32; 5] = [
        0.254_829_6,
        -0.284_496_74,
        1.421_413_8,
        -1.453_152,
        1.061_405_4,
    ];
    let t = 1.0 / P.mul_add(x.abs(), 1.0);
    let polynomial = A.iter().rev().fold(0.0_f32, |sum, &a| sum.mul_add(t, a)) * t;
    polynomial.mul_add(-(-x * x).exp(), 1.0).copysign(x)
}

/// Adds a vertex at `pos` in `colour` at `alpha` of its opacity, and returns its index.
fn vertex(mesh: &mut Mesh, pos: Pos2, colour: Color32, alpha: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a component's mesh holds a few thousand vertices, far below u32's range"
    )]
    let index = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        pos,
        uv: WHITE_UV,
        color: colour.gamma_multiply(alpha),
    });
    index
}

/// How many segments approximate a curve of `length` points: one every 2 points, at least
/// `least`.
fn segments(length: f32, least: u32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a component's curves are a few thousand points long at most, and the length is \
                  clamped to be positive"
    )]
    let segments = (length.max(0.0) / 2.0).ceil() as u32;
    segments.max(least)
}

/// Adds an arc around `centre` at `radius`, from the angle `from` to `to` in radians clockwise
/// from the right, with round caps, its cross-section `profile` in `colour`.
pub fn stroke_arc(
    mesh: &mut Mesh,
    centre: Pos2,
    radius: f32,
    (from, to): (f32, f32),
    profile: &[Stop],
    colour: Color32,
) {
    // The cross-section's offsets from the centre line, outside to inside.
    let across: Vec<Stop> = profile
        .iter()
        .rev()
        .copied()
        .chain(profile.iter().skip(1).map(|stop| Stop {
            distance: -stop.distance,
            alpha: stop.alpha,
        }))
        .collect();
    let Some(columns) = u32::try_from(across.len()).ok().filter(|&n| n >= 2) else {
        return;
    };
    let rows = segments((to - from).abs() * radius, 2);
    let first = vertex_count(mesh);
    for row in 0..=rows {
        let angle = (to - from).mul_add(fraction(row, rows), from);
        let direction = vec2(angle.cos(), angle.sin());
        for stop in &across {
            vertex(
                mesh,
                centre + direction * (radius + stop.distance),
                colour,
                stop.alpha,
            );
        }
    }
    for row in 0..rows {
        for column in 0..columns - 1 {
            let corner = first + row * columns + column;
            mesh.add_triangle(corner, corner + 1, corner + columns);
            mesh.add_triangle(corner + 1, corner + columns + 1, corner + columns);
        }
    }
    // Each cap is a half disk turned outward from the arc's end.
    for (angle, outward) in [(from, -1.0), (to, 1.0)] {
        let normal = vec2(angle.cos(), angle.sin());
        let tangent = vec2(-angle.sin(), angle.cos()) * outward;
        fan(
            mesh,
            centre + normal * radius,
            profile,
            colour,
            |turn| normal * turn.cos() + tangent * turn.sin(),
            PI,
        );
    }
}

/// Adds a disk around `centre`, its cross-section `profile` in `colour`.
pub fn disk(mesh: &mut Mesh, centre: Pos2, profile: &[Stop], colour: Color32) {
    fan(
        mesh,
        centre,
        profile,
        colour,
        |turn| vec2(turn.cos(), turn.sin()),
        TAU,
    );
}

/// Adds rings around `centre` at each stop's distance, swept through `sweep` radians of
/// `direction`.
fn fan(
    mesh: &mut Mesh,
    centre: Pos2,
    profile: &[Stop],
    colour: Color32,
    direction: impl Fn(f32) -> Vec2,
    sweep: f32,
) {
    let (Some(inner), Some(outer)) = (profile.first(), profile.last()) else {
        return;
    };
    let Some(rings) = u32::try_from(profile.len() - 1).ok().filter(|&n| n >= 1) else {
        return;
    };
    let steps = segments(sweep * outer.distance, 8).min(96);
    let middle = vertex(mesh, centre, colour, inner.alpha);
    let first = vertex_count(mesh);
    for step in 0..=steps {
        let direction = direction(sweep * fraction(step, steps));
        for stop in profile.iter().skip(1) {
            vertex(mesh, centre + direction * stop.distance, colour, stop.alpha);
        }
    }
    for step in 0..steps {
        let this = first + step * rings;
        let next = this + rings;
        mesh.add_triangle(middle, this, next);
        for ring in 0..rings - 1 {
            mesh.add_triangle(this + ring, this + ring + 1, next + ring);
            mesh.add_triangle(this + ring + 1, next + ring + 1, next + ring);
        }
    }
}

/// The index the next vertex added to `mesh` gets.
const fn vertex_count(mesh: &Mesh) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a component's mesh holds a few thousand vertices, far below u32's range"
    )]
    let count = mesh.vertices.len() as u32;
    count
}

/// `step` out of `steps`, as a fraction from 0 to 1.
const fn fraction(step: u32, steps: u32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "segment counts are a few hundred at most, exact in f32"
    )]
    let fraction = step as f32 / steps as f32;
    fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn erf_matches_known_values() {
        assert_close(erf(0.0), 0.0, 1e-6);
        assert_close(erf(0.5), 0.520_499_9, 1e-6);
        assert_close(erf(1.0), 0.842_700_8, 1e-6);
        assert_close(erf(-1.0), -0.842_700_8, 1e-6);
        assert_close(erf(3.0), 0.999_977_9, 1e-6);
    }

    #[test]
    fn a_crisp_edge_fades_over_one_pixel_around_the_edge() {
        let profile = crisp(6.0, 0.5);

        assert_eq!(profile.len(), 3);
        assert_close(profile[1].distance, 5.75, 1e-6);
        assert_close(profile[2].distance, 6.25, 1e-6);
        assert_close(profile[2].alpha, 0.0, 0.0);
    }

    #[test]
    fn a_glow_is_half_strength_at_the_shapes_edge_and_fades_to_nothing_at_its_reach() {
        let glow = Glow {
            sigma: 4.0,
            spread: 0.0,
            opacity: 1.0,
        };
        let profile = glow.profile(20.0);

        let at_edge = profile
            .iter()
            .find(|stop| (stop.distance - 20.0).abs() < 0.6)
            .expect("a stop near the edge");
        assert_close(at_edge.alpha, 0.5, 0.06);
        assert_close(profile[0].alpha, 1.0, 1e-3);
        let last = profile.last().expect("stops");
        assert_close(last.distance, 32.0, 1e-4);
        assert!(last.alpha < 0.002, "{last:?}");
        assert!(
            profile
                .windows(2)
                .all(|pair| pair[1].alpha <= pair[0].alpha),
            "{profile:?}"
        );
    }

    #[test]
    fn a_glow_keeps_its_proportions_at_every_scale() {
        let glow = Glow {
            sigma: 4.5,
            spread: 3.0,
            opacity: 0.8,
        };
        let scaled = Glow {
            sigma: 4.5 * 2.5,
            spread: 3.0 * 2.5,
            ..glow
        };

        for (stop, scaled_stop) in glow.profile(6.0).iter().zip(&scaled.profile(15.0)) {
            assert_close(scaled_stop.distance, stop.distance * 2.5, 1e-4);
            assert_close(scaled_stop.alpha, stop.alpha, 1e-5);
        }
    }

    #[test]
    fn an_arc_and_a_disk_make_whole_triangles() {
        let mut mesh = Mesh::default();
        stroke_arc(
            &mut mesh,
            Pos2::new(50.0, 50.0),
            40.0,
            (0.75 * PI, 2.0 * PI),
            &crisp(6.0, 1.0),
            Color32::WHITE,
        );
        disk(
            &mut mesh,
            Pos2::new(10.0, 10.0),
            &crisp(2.0, 1.0),
            Color32::WHITE,
        );

        assert!(mesh.is_valid());
        assert_eq!(mesh.indices.len() % 3, 0);
    }
}
