#pragma once
#include "rust/cxx.h"
#include <TopoDS_Shape.hxx>
#include <memory>

namespace cadet {

struct Seg;
struct MeshOut;
struct Lines;
struct RayHit;
struct Props;

using Shape = std::unique_ptr<TopoDS_Shape>;

Shape wire(rust::Slice<const Seg> segs);
Shape face(const TopoDS_Shape &wire);
Shape cut(const TopoDS_Shape &a, const TopoDS_Shape &b);
Shape fuse(const TopoDS_Shape &a, const TopoDS_Shape &b);
Shape prism(const TopoDS_Shape &s, double x, double y, double z);
Shape revol(const TopoDS_Shape &s, rust::Slice<const double> origin, rust::Slice<const double> dir, double angle);
Shape translated(const TopoDS_Shape &s, double x, double y, double z);
Shape round_edges(const TopoDS_Shape &s, rust::Slice<const uint32_t> edges, double size, bool chamfer);
Shape compound();
void compound_add(TopoDS_Shape &c, const TopoDS_Shape &s);
Lines edge_lines(const TopoDS_Shape &s, double tol);
MeshOut mesh(const TopoDS_Shape &s, double tol);
Props props(const TopoDS_Shape &s);
RayHit ray(const TopoDS_Shape &s, rust::Slice<const double> origin, rust::Slice<const double> dir);
void write_stl(const TopoDS_Shape &s, rust::Str path, double tol);
void write_step(const TopoDS_Shape &s, rust::Str path);

} // namespace cadet
