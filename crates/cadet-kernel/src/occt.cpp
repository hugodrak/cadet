// Thin, exception-safe wrappers over OCCT. Every OCCT failure becomes a std::runtime_error,
// which cxx turns into a Rust Err instead of aborting the process.

#include "cadet-kernel/src/occt.h"
#include "cadet-kernel/src/ffi.rs.h"

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBndLib.hxx>
#include <BRepGProp.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GProp_GProps.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepIntCurveSurface_Inter.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <Standard_Failure.hxx>
#include <StlAPI_Writer.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <gp_Circ.hxx>
#include <gp_Lin.hxx>
#include <gp_Pln.hxx>

#include <stdexcept>
#include <string>

namespace cadet {

template <class F> static auto guard(const char *what, F f) -> decltype(f()) {
  try {
    return f();
  } catch (const Standard_Failure &e) {
    throw std::runtime_error(std::string(what) + ": " + e.GetMessageString());
  } catch (const std::exception &) {
    throw;
  } catch (...) {
    throw std::runtime_error(std::string(what) + " failed");
  }
}

static Shape own(const TopoDS_Shape &s) { return std::make_unique<TopoDS_Shape>(s); }
static gp_Pnt pnt(const double *p) { return gp_Pnt(p[0], p[1], p[2]); }

static TopoDS_Shape unify(const TopoDS_Shape &s) {
  ShapeUpgrade_UnifySameDomain u(s, true, true, true);
  u.AllowInternalEdges(false);
  u.Build();
  return u.Shape();
}

Shape wire(rust::Slice<const Seg> segs) {
  return guard("wire", [&] {
    BRepBuilderAPI_MakeWire w;
    for (const Seg &s : segs) {
      const double *p = s.p.data();
      if (s.kind == 0) {
        w.Add(BRepBuilderAPI_MakeEdge(pnt(p), pnt(p + 3)).Edge());
      } else if (s.kind == 1) {
        GC_MakeArcOfCircle arc(pnt(p), pnt(p + 3), pnt(p + 6));
        w.Add(BRepBuilderAPI_MakeEdge(arc.Value()).Edge());
      } else {
        gp_Circ c(gp_Ax2(pnt(p), gp_Dir(p[3], p[4], p[5])), s.r);
        w.Add(BRepBuilderAPI_MakeEdge(c).Edge());
      }
    }
    if (!w.IsDone()) throw std::runtime_error("the profile isn't a connected loop");
    return own(w.Wire());
  });
}

Shape face(const TopoDS_Shape &w) {
  return guard("face", [&] {
    BRepBuilderAPI_MakeFace f(TopoDS::Wire(w), true);
    if (!f.IsDone()) throw std::runtime_error("the profile isn't flat");
    return own(f.Face());
  });
}

Shape cut(const TopoDS_Shape &a, const TopoDS_Shape &b) {
  return guard("cut", [&] {
    BRepAlgoAPI_Cut op(a, b);
    if (op.HasErrors()) throw std::runtime_error("the boolean cut failed");
    return own(unify(op.Shape()));
  });
}

Shape fuse(const TopoDS_Shape &a, const TopoDS_Shape &b) {
  return guard("join", [&] {
    BRepAlgoAPI_Fuse op(a, b);
    if (op.HasErrors()) throw std::runtime_error("the boolean join failed");
    return own(unify(op.Shape()));
  });
}

Shape prism(const TopoDS_Shape &s, double x, double y, double z) {
  return guard("extrude", [&] {
    BRepPrimAPI_MakePrism op(s, gp_Vec(x, y, z));
    if (!op.IsDone()) throw std::runtime_error("the extrusion failed");
    return own(op.Shape());
  });
}

Shape revol(const TopoDS_Shape &s, rust::Slice<const double> o, rust::Slice<const double> d, double angle) {
  return guard("revolve", [&] {
    BRepPrimAPI_MakeRevol op(s, gp_Ax1(pnt(o.data()), gp_Dir(d[0], d[1], d[2])), angle);
    if (!op.IsDone()) throw std::runtime_error("the revolution failed");
    return own(op.Shape());
  });
}

Shape translated(const TopoDS_Shape &s, double x, double y, double z) {
  return guard("move", [&] {
    gp_Trsf t;
    t.SetTranslation(gp_Vec(x, y, z));
    return own(BRepBuilderAPI_Transform(s, t, true).Shape());
  });
}

Shape round_edges(const TopoDS_Shape &s, rust::Slice<const uint32_t> edges, double size, bool chamfer) {
  return guard(chamfer ? "chamfer" : "fillet", [&] {
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(s, TopAbs_EDGE, map);
    TopoDS_Shape out;
    if (chamfer) {
      BRepFilletAPI_MakeChamfer op(s);
      for (uint32_t i : edges) op.Add(size, TopoDS::Edge(map(int(i) + 1)));
      op.Build();
      if (!op.IsDone()) throw std::runtime_error("the chamfer doesn't fit; try a smaller distance");
      out = op.Shape();
    } else {
      BRepFilletAPI_MakeFillet op(s);
      for (uint32_t i : edges) op.Add(size, TopoDS::Edge(map(int(i) + 1)));
      op.Build();
      if (!op.IsDone()) throw std::runtime_error("the fillet doesn't fit; try a smaller radius");
      out = op.Shape();
    }
    return own(out);
  });
}

Shape compound() {
  TopoDS_Compound c;
  BRep_Builder().MakeCompound(c);
  return own(c);
}

void compound_add(TopoDS_Shape &c, const TopoDS_Shape &s) {
  guard("compound", [&] { BRep_Builder().Add(c, s); });
}

Lines edge_lines(const TopoDS_Shape &s, double tol) {
  return guard("edges", [&] {
    Lines out;
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(s, TopAbs_EDGE, map);
    for (int i = 1; i <= map.Extent(); ++i) {
      const TopoDS_Edge &e = TopoDS::Edge(map(i));
      uint32_t n = 0;
      EdgeInfo info{};
      if (!BRep_Tool::Degenerated(e)) {
        BRepAdaptor_Curve c(e);
        info.length = GCPnts_AbscissaPoint::Length(c);
        if (c.GetType() == GeomAbs_Circle) {
          gp_Circ circ = c.Circle();
          info.circle = true;
          info.center = {circ.Location().X(), circ.Location().Y(), circ.Location().Z()};
          info.radius = circ.Radius();
        }
        GCPnts_TangentialDeflection pts(c, 0.1, tol);
        for (int k = 1; k <= pts.NbPoints(); ++k, ++n) {
          gp_Pnt p = pts.Value(k);
          out.xyz.push_back(p.X());
          out.xyz.push_back(p.Y());
          out.xyz.push_back(p.Z());
        }
      }
      out.counts.push_back(n);
      out.info.push_back(info);
    }
    return out;
  });
}

MeshOut mesh(const TopoDS_Shape &s, double tol) {
  return guard("mesh", [&] {
    MeshOut out;
    BRepMesh_IncrementalMesh(s, tol, false, 0.3, true);
    for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
      const TopoDS_Face &f = TopoDS::Face(ex.Current());
      TopLoc_Location loc;
      Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(f, loc);
      if (tri.IsNull()) continue;
      BRepLib_ToolTriangulatedShape::ComputeNormals(f, tri);
      bool flip = f.Orientation() == TopAbs_REVERSED;
      gp_Trsf t = loc.Transformation();
      uint32_t base = uint32_t(out.pos.size() / 3);
      for (int i = 1; i <= tri->NbNodes(); ++i) {
        gp_Pnt p = tri->Node(i).Transformed(t);
        gp_Dir n = tri->Normal(i).Transformed(t);
        if (flip) n.Reverse();
        out.pos.push_back(float(p.X()));
        out.pos.push_back(float(p.Y()));
        out.pos.push_back(float(p.Z()));
        out.nrm.push_back(float(n.X()));
        out.nrm.push_back(float(n.Y()));
        out.nrm.push_back(float(n.Z()));
      }
      for (int i = 1; i <= tri->NbTriangles(); ++i) {
        int a, b, c;
        tri->Triangle(i).Get(a, b, c);
        if (flip) std::swap(b, c);
        out.idx.push_back(base + a - 1);
        out.idx.push_back(base + b - 1);
        out.idx.push_back(base + c - 1);
      }
    }
    return out;
  });
}

Props props(const TopoDS_Shape &s) {
  return guard("measure", [&] {
    Props out{};
    Bnd_Box box;
    BRepBndLib::AddOptimal(s, box, false, false);
    if (!box.IsVoid()) {
      double x0, y0, z0, x1, y1, z1;
      box.Get(x0, y0, z0, x1, y1, z1);
      out.min = {x0, y0, z0};
      out.max = {x1, y1, z1};
    }
    GProp_GProps vol, area;
    BRepGProp::VolumeProperties(s, vol);
    BRepGProp::SurfaceProperties(s, area);
    out.volume = vol.Mass();
    out.area = area.Mass();
    return out;
  });
}

RayHit ray(const TopoDS_Shape &s, rust::Slice<const double> o, rust::Slice<const double> d) {
  return guard("ray", [&] {
    RayHit hit{};
    BRepIntCurveSurface_Inter inter;
    inter.Init(s, gp_Lin(pnt(o.data()), gp_Dir(d[0], d[1], d[2])), 1e-7);
    for (; inter.More(); inter.Next()) {
      double w = inter.W();
      if (w <= 1e-9 || (hit.hit && w >= hit.t)) continue;
      const TopoDS_Face &f = inter.Face();
      gp_Pnt p = inter.Pnt();
      hit.hit = true;
      hit.t = w;
      hit.p = {p.X(), p.Y(), p.Z()};
      BRepAdaptor_Surface surf(f);
      hit.planar = surf.GetType() == GeomAbs_Plane;
      if (hit.planar) {
        gp_Dir n = surf.Plane().Axis().Direction();
        if (f.Orientation() == TopAbs_REVERSED) n.Reverse();
        hit.n = {n.X(), n.Y(), n.Z()};
      }
    }
    return hit;
  });
}

void write_stl(const TopoDS_Shape &s, rust::Str path, double tol) {
  guard("STL export", [&] {
    BRepMesh_IncrementalMesh(s, tol, false, 0.1, true);
    StlAPI_Writer w;
    w.ASCIIMode() = Standard_False;
    if (!w.Write(s, std::string(path).c_str())) throw std::runtime_error("couldn't write the file");
  });
}

void write_step(const TopoDS_Shape &s, rust::Str path) {
  guard("STEP export", [&] {
    STEPControl_Writer w;
    if (w.Transfer(s, STEPControl_AsIs) != IFSelect_RetDone) throw std::runtime_error("couldn't convert the model");
    if (w.Write(std::string(path).c_str()) != IFSelect_RetDone) throw std::runtime_error("couldn't write the file");
  });
}

} // namespace cadet
