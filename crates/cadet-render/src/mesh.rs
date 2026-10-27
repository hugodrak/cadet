use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Default, Clone)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub edges: Vec<[f32; 3]>,
}

impl MeshData {
    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        let mut m = Self::default();
        let c = |i: usize| Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        );
        let faces = [
            ([0, 2, 3, 1], -Vec3::Z), ([4, 5, 7, 6], Vec3::Z),
            ([0, 1, 5, 4], -Vec3::Y), ([2, 6, 7, 3], Vec3::Y),
            ([0, 4, 6, 2], -Vec3::X), ([1, 3, 7, 5], Vec3::X),
        ];
        for (q, n) in faces {
            let base = m.vertices.len() as u32;
            for &i in &q {
                m.vertices.push(Vertex { pos: c(i).into(), normal: n.into() });
            }
            m.indices.extend([0, 1, 2, 0, 2, 3].map(|k| base + k));
        }
        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            m.edges.push(c(a).into());
            m.edges.push(c(b).into());
        }
        m
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.vertices.iter().map(|v| Vec3::from(v.pos));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
}
