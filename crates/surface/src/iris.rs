//! Sculpture native décorative : géométrie préparée une fois, éclairage et rotation bornés.
//! Elle ne représente aucune télémétrie. Aucune texture distante ni allocation par pixel.

use egui::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use std::f32::consts::TAU;

type V3 = [f32; 3];
const LONGITUDES: usize = 192;
const LATITUDES: usize = 40;

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(a: V3) -> V3 {
    mul(a, dot(a, a).sqrt().recip())
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn curve(t: f32) -> V3 {
    let r = 0.90 + 0.30 * (3.0 * t).cos();
    [
        r * (2.0 * t).cos(),
        r * (2.0 * t).sin(),
        0.42 * (3.0 * t).sin(),
    ]
}
fn rotate(p: V3, phase: f32) -> V3 {
    let (s, c) = (0.85_f32).sin_cos();
    let p = [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c];
    let (s, c) = (0.52 + phase).sin_cos();
    let p = [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c];
    let (s, c) = (-0.38_f32).sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
}

pub(crate) struct Iris {
    vertices: Vec<(V3, V3, f32)>,
    faces: Vec<[u32; 4]>,
}

impl Default for Iris {
    fn default() -> Self {
        let mut vertices = Vec::with_capacity((LONGITUDES + 1) * (LATITUDES + 1));
        for u in 0..=LONGITUDES {
            let t = u as f32 / LONGITUDES as f32 * TAU;
            let center = curve(t);
            let tangent = unit(add(curve(t + 0.001), mul(curve(t - 0.001), -1.0)));
            let radial = unit([center[0], center[1], 0.0]);
            let binormal = unit(cross(tangent, radial));
            let normal = unit(cross(binormal, tangent));
            for v in 0..=LATITUDES {
                let a = v as f32 / LATITUDES as f32 * TAU;
                let n = add(mul(normal, a.cos()), mul(binormal, a.sin()));
                vertices.push((add(center, mul(n, 0.245)), n, t));
            }
        }
        let mut faces = Vec::with_capacity(LONGITUDES * LATITUDES);
        for u in 0..LONGITUDES {
            for v in 0..LATITUDES {
                let i = (u * (LATITUDES + 1) + v) as u32;
                let j = i + LATITUDES as u32 + 1;
                faces.push([i, j, j + 1, i + 1]);
            }
        }
        Self { vertices, faces }
    }
}

impl Iris {
    pub(crate) fn paint(&self, painter: &egui::Painter, rect: Rect, time: f32) {
        let center = rect.center();
        let scale = rect.width().min(rect.height()) * 0.34;
        glow(
            painter,
            center + vec2(-scale * 0.20, 0.0),
            vec2(scale * 1.8, scale * 1.4),
            [100, 63, 236],
            75,
        );
        glow(
            painter,
            center + vec2(scale * 0.6, scale * 0.4),
            vec2(scale * 1.25, scale),
            [36, 161, 193],
            35,
        );
        let phase = time * 0.055;
        let light = unit([-0.5, -0.8, 1.5]);
        let rim = unit([1.0, 0.7, 0.8]);
        let half = unit(add(light, [0.0, 0.0, 1.0]));
        let mut mesh = egui::Mesh::default();
        let mut depths = Vec::with_capacity(self.vertices.len());
        let projected: Vec<_> = self
            .vertices
            .iter()
            .map(|&(p, normal, t)| {
                let p = rotate(p, phase);
                let perspective = 4.8 / (4.8 - p[2]);
                (
                    vec2(p[0], p[1]) * perspective,
                    rotate(normal, phase),
                    t,
                    p[2],
                )
            })
            .collect();
        let mut bounds = Rect::NOTHING;
        for &(point, _, _, _) in &projected {
            bounds.extend_with(pos2(point.x, point.y));
        }
        // Le cadrage tient compte de la perspective à chaque orientation ; aucune
        // extrémité ne doit être coupée ni entrer dans la zone de saisie.
        let scale = (rect.width() / bounds.width()).min(rect.height() / bounds.height()) * 0.92;
        let center = rect.center() - bounds.center().to_vec2() * scale;
        for (point, n, t, depth) in projected {
            let diffuse = dot(n, light).max(0.0);
            let edge = dot(n, rim).max(0.0).powf(3.0);
            let specular = dot(n, half).max(0.0).powf(60.0);
            let fresnel = (1.0 - n[2].abs()).powf(3.0);
            let iris = ((t * 1.4 + n[1] * 2.0).sin() * 0.5 + 0.5).powf(1.5);
            let base = [
                105.0 + iris * 60.0,
                68.0 + iris * 128.0,
                222.0 + iris * 20.0,
            ];
            let color = |i: usize| {
                (base[i] * (0.23 + 0.70 * diffuse)
                    + 145.0 * specular
                    + [28.0, 65.0, 70.0][i] * edge
                    + 65.0 * fresnel)
                    .clamp(0.0, 255.0) as u8
            };
            mesh.colored_vertex(
                center + point * scale,
                Color32::from_rgb(color(0), color(1), color(2)),
            );
            depths.push(depth);
        }
        let mut ordered: Vec<_> = self
            .faces
            .iter()
            .map(|face| {
                let z = face.iter().map(|i| depths[*i as usize]).sum::<f32>();
                (z, face)
            })
            .collect();
        ordered.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        for (_, &[a, b, c, d]) in ordered {
            mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}

pub(crate) fn glow(painter: &egui::Painter, center: Pos2, radius: Vec2, rgb: [u8; 3], alpha: u8) {
    let mut mesh = egui::Mesh::default();
    const RINGS: u32 = 12;
    const SEGMENTS: u32 = 64;
    for ring in 0..=RINGS {
        let r = ring as f32 / RINGS as f32;
        let opacity = ((-r * r * 5.5).exp() - (-5.5_f32).exp()).max(0.0) * f32::from(alpha);
        for segment in 0..=SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * TAU;
            mesh.colored_vertex(
                center + vec2(angle.cos() * radius.x, angle.sin() * radius.y) * r,
                Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], opacity as u8),
            );
            if ring < RINGS && segment < SEGMENTS {
                let a = ring * (SEGMENTS + 1) + segment;
                let b = a + SEGMENTS + 1;
                mesh.indices
                    .extend_from_slice(&[a, b, b + 1, a, b + 1, a + 1]);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Home,
    Chat,
    Models,
    Activity,
    Arrow,
    Spark,
}

pub(crate) fn icon(painter: &egui::Painter, center: Pos2, kind: Icon, size: f32, color: Color32) {
    let p = |x: f32, y: f32| center + vec2(x, y) * size;
    let stroke = Stroke::new(1.5, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
    };
    match kind {
        Icon::Home => {
            for (x, y) in [(-0.45, -0.45), (0.12, -0.45), (-0.45, 0.12), (0.12, 0.12)] {
                painter.rect_stroke(
                    Rect::from_min_size(p(x, y), vec2(size * 0.33, size * 0.33)),
                    2,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Chat => {
            painter.rect_stroke(
                Rect::from_min_max(p(-0.47, -0.38), p(0.47, 0.29)),
                4,
                stroke,
                egui::StrokeKind::Inside,
            );
            line((-0.28, 0.29), (-0.28, 0.48));
            line((-0.28, 0.48), (-0.05, 0.29));
            line((-0.22, -0.08), (0.22, -0.08));
        }
        Icon::Models => {
            painter.rect_stroke(
                Rect::from_min_max(p(-0.30, -0.30), p(0.30, 0.30)),
                3,
                stroke,
                egui::StrokeKind::Inside,
            );
            for n in [-0.16, 0.16] {
                line((n, -0.5), (n, -0.30));
                line((n, 0.30), (n, 0.5));
                line((-0.5, n), (-0.30, n));
                line((0.30, n), (0.5, n));
            }
        }
        Icon::Activity => {
            painter.add(egui::Shape::line(
                vec![
                    p(-0.5, 0.1),
                    p(-0.25, 0.1),
                    p(-0.07, -0.4),
                    p(0.13, 0.4),
                    p(0.30, -0.1),
                    p(0.5, -0.1),
                ],
                stroke,
            ));
        }
        Icon::Arrow => {
            line((-0.3, 0.3), (0.3, -0.3));
            line((-0.3, -0.3), (0.3, -0.3));
            line((0.3, -0.3), (0.3, 0.3));
        }
        Icon::Spark => {
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                painter.line_segment(
                    [
                        center + vec2(a.cos(), a.sin()) * size * 0.14,
                        center + vec2(a.cos(), a.sin()) * size * 0.5,
                    ],
                    Stroke::new(2.0, color),
                );
            }
            painter.circle_filled(pos2(center.x, center.y), size * 0.08, color);
        }
    }
}
