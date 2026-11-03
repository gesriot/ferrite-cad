// SPDX-License-Identifier: MIT
#pragma once

#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepPrimAPI_MakeHalfSpace.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Solid.hxx>
#include <stdexcept>

namespace next_boundary {

// The directional derivative of an implicit quadric is affine in the point.
// Intersect the actual trimmed face with its negative half-space. Only a face
// in the intersection witnesses back-facing area; a tangent edge is allowed.
template <class Quadric>
bool forward_quadric(const Quadric &surface, const TopoDS_Face &face, const gp_Dir &direction) {
  double xx, yy, zz, xy, xz, yz, x, y, z, constant;
  surface.Coefficients(xx, yy, zz, xy, xz, yz, x, y, z, constant);
  const auto d = direction.XYZ();
  const gp_Vec gradient(xx * d.X() + xy * d.Y() + xz * d.Z(),
                        xy * d.X() + yy * d.Y() + yz * d.Z(),
                        xz * d.X() + yz * d.Y() + zz * d.Z());
  const double offset = gp_XYZ(x, y, z).Dot(d);
  const double orientation = (surface.Direct() == (face.Orientation() == TopAbs_FORWARD))
      ? 1. : -1.;
  if (gradient.SquareMagnitude() == 0.) return orientation * offset >= 0.;
  const gp_Dir positive(gradient * orientation);
  const gp_Pnt on_plane(gradient.XYZ() * (-offset / gradient.SquareMagnitude()));
  const auto plane = BRepBuilderAPI_MakeFace(gp_Pln(on_plane, positive)).Face();
  auto halfspace = BRepPrimAPI_MakeHalfSpace(plane, on_plane.Translated(-gp_Vec(positive)));
  BRepAlgoAPI_Common overlap(face, halfspace.Solid());
  if (!overlap.IsDone()) throw std::runtime_error("cannot check the complete trimmed end normal");
  return !TopExp_Explorer(overlap.Shape(), TopAbs_FACE).More();
}

inline bool forward_patch(const TopoDS_Face &face, const gp_Dir &direction) {
  const BRepAdaptor_Surface surface(face);
  switch (surface.GetType()) {
  case GeomAbs_Plane: {
    const auto plane = surface.Plane();
    gp_Dir normal = plane.Axis().Direction();
    if (!plane.Direct()) normal.Reverse();
    if (face.Orientation() == TopAbs_REVERSED) normal.Reverse();
    return normal.Dot(direction) >= 0.;
  }
  case GeomAbs_Cylinder: return forward_quadric(surface.Cylinder(), face, direction);
  case GeomAbs_Sphere: return forward_quadric(surface.Sphere(), face, direction);
  case GeomAbs_Cone: return forward_quadric(surface.Cone(), face, direction);
  default:
    throw std::runtime_error(
        "cannot certify the normal direction on the complete trimmed end face");
  }
}
} // namespace next_boundary
