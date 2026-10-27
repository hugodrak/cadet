#[cxx::bridge(namespace = "cadet")]
pub mod occt {
    /// kind 0: line (p[0..3] → p[3..6]); 1: arc through p[0..3], p[3..6], p[6..9]; 2: circle at p[0..3], normal p[3..6], radius r.
    struct Seg {
        kind: u8,
        p: [f64; 9],
        r: f64,
    }

    struct MeshOut {
        pos: Vec<f32>,
        nrm: Vec<f32>,
        idx: Vec<u32>,
    }

    /// Edge polylines, in TopExp::MapShapes order, plus exact per-edge data.
    struct Lines {
        xyz: Vec<f64>,
        counts: Vec<u32>,
        info: Vec<EdgeInfo>,
    }

    struct EdgeInfo {
        length: f64,
        circle: bool,
        center: [f64; 3],
        radius: f64,
    }

    /// Exact bounding box and mass properties.
    struct Props {
        min: [f64; 3],
        max: [f64; 3],
        volume: f64,
        area: f64,
    }

    struct RayHit {
        hit: bool,
        t: f64,
        p: [f64; 3],
        planar: bool,
        n: [f64; 3],
    }

    unsafe extern "C++" {
        include!("cadet-kernel/src/occt.h");

        #[namespace = ""]
        type TopoDS_Shape;

        fn wire(segs: &[Seg]) -> Result<UniquePtr<TopoDS_Shape>>;
        fn face(wire: &TopoDS_Shape) -> Result<UniquePtr<TopoDS_Shape>>;
        fn cut(a: &TopoDS_Shape, b: &TopoDS_Shape) -> Result<UniquePtr<TopoDS_Shape>>;
        fn fuse(a: &TopoDS_Shape, b: &TopoDS_Shape) -> Result<UniquePtr<TopoDS_Shape>>;
        fn prism(s: &TopoDS_Shape, x: f64, y: f64, z: f64) -> Result<UniquePtr<TopoDS_Shape>>;
        fn revol(s: &TopoDS_Shape, origin: &[f64], dir: &[f64], angle: f64) -> Result<UniquePtr<TopoDS_Shape>>;
        fn translated(s: &TopoDS_Shape, x: f64, y: f64, z: f64) -> Result<UniquePtr<TopoDS_Shape>>;
        fn round_edges(s: &TopoDS_Shape, edges: &[u32], size: f64, chamfer: bool) -> Result<UniquePtr<TopoDS_Shape>>;
        fn compound() -> UniquePtr<TopoDS_Shape>;
        fn compound_add(c: Pin<&mut TopoDS_Shape>, s: &TopoDS_Shape) -> Result<()>;
        fn edge_lines(s: &TopoDS_Shape, tol: f64) -> Result<Lines>;
        fn mesh(s: &TopoDS_Shape, tol: f64) -> Result<MeshOut>;
        fn props(s: &TopoDS_Shape) -> Result<Props>;
        fn ray(s: &TopoDS_Shape, origin: &[f64], dir: &[f64]) -> Result<RayHit>;
        fn write_stl(s: &TopoDS_Shape, path: &str, tol: f64) -> Result<()>;
        fn write_step(s: &TopoDS_Shape, path: &str) -> Result<()>;
        fn read_cad(path: &str, iges: bool) -> Result<UniquePtr<TopoDS_Shape>>;
        fn to_brep(s: &TopoDS_Shape) -> Result<String>;
        fn from_brep(text: &str) -> Result<UniquePtr<TopoDS_Shape>>;
        fn solid_count(s: &TopoDS_Shape) -> usize;
        fn solid(s: &TopoDS_Shape, i: usize) -> Result<UniquePtr<TopoDS_Shape>>;
    }
}
