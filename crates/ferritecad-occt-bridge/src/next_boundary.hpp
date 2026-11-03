// SPDX-License-Identifier: MIT
#pragma once

#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepPrimAPI_MakeHalfSpace.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Solid.hxx>
#include <Precision.hxx>
#include <algorithm>
#include <cmath>
#include <stdexcept>
#include <string>
#include "ferritecad_occt.h"

namespace next_boundary {

// The directional derivative of an implicit quadric is affine in the point.
// Intersect the actual trimmed face with its negative half-space. Only a face
// in the intersection witnesses back-facing area; a tangent edge is allowed.
template <class Quadric>
bool forward_quadric(const Quadric &surface, const TopoDS_Face &face, const gp_Dir &direction) {
  double xx, yy, zz, xy, xz, yz, x, y, z, constant;
  // In the quadric frame the quadratic matrix is diagonal. Its largest
  // absolute diagonal coefficient scales the parallel test independently of
  // both rotation and the choice of model origin.
  auto canonical = surface;
  canonical.SetPosition(gp_Ax3());
  canonical.Coefficients(xx, yy, zz, xy, xz, yz, x, y, z, constant);
  const auto &axes = surface.Position();
  const double dx = direction.Dot(axes.XDirection()), dy = direction.Dot(axes.YDirection());
  const double dz = direction.Dot(axes.Direction());
  const gp_Vec gradient = gp_Vec(axes.XDirection()) * (xx * dx)
                        + gp_Vec(axes.YDirection()) * (yy * dy)
                        + gp_Vec(axes.Direction()) * (zz * dz);
  const double offset = x * dx + y * dy + z * dz;
  const double orientation = (surface.Direct() == (face.Orientation() == TopAbs_FORWARD))
      ? 1. : -1.;
  const double scale = std::max({std::abs(xx), std::abs(yy), std::abs(zz)});
  if (gradient.Magnitude() <= scale * Precision::Angular()) return orientation * offset >= 0.;
  const gp_Dir positive(gradient * orientation);
  const gp_Pnt on_plane =
      axes.Location().Translated(gradient * (-offset / gradient.SquareMagnitude()));
  const auto plane = BRepBuilderAPI_MakeFace(gp_Pln(on_plane, positive)).Face();
  auto halfspace = BRepPrimAPI_MakeHalfSpace(plane, on_plane.Translated(-gp_Vec(positive)));
  BRepAlgoAPI_Common overlap(face, halfspace.Solid());
  if (!overlap.IsDone()) throw std::runtime_error("cannot check the complete trimmed end normal");
  return !TopExp_Explorer(overlap.Shape(), TopAbs_FACE).More();
}

inline FcOcctStatus check_patch(const TopoDS_Face &face, const gp_Dir &direction,
                                std::string &reason) {
  const BRepAdaptor_Surface surface(face);
  const char *unsupported = nullptr;
  bool forward = false;
  switch (surface.GetType()) {
  case GeomAbs_Plane: {
    const auto plane = surface.Plane();
    gp_Dir normal = plane.Axis().Direction();
    if (!plane.Direct()) normal.Reverse();
    if (face.Orientation() == TopAbs_REVERSED) normal.Reverse();
    forward = normal.IsNormal(direction, Precision::Angular()) || normal.Dot(direction) > 0.;
    break;
  }
  case GeomAbs_Cylinder: forward = forward_quadric(surface.Cylinder(), face, direction); break;
  case GeomAbs_Sphere: forward = forward_quadric(surface.Sphere(), face, direction); break;
  case GeomAbs_Cone: forward = forward_quadric(surface.Cone(), face, direction); break;
  case GeomAbs_Torus: unsupported = "a toroidal face"; break;
  case GeomAbs_BSplineSurface: unsupported = "a B-spline face"; break;
  case GeomAbs_BezierSurface: unsupported = "a Bézier face"; break;
  case GeomAbs_SurfaceOfRevolution: unsupported = "a surface of revolution"; break;
  case GeomAbs_SurfaceOfExtrusion: unsupported = "a surface of extrusion"; break;
  case GeomAbs_OffsetSurface: unsupported = "an offset surface"; break;
  default: unsupported = "an unclassified surface"; break;
  }
  if (unsupported) {
    reason = std::string("up to next supports planar, cylindrical, conical and spherical ")
             + "end faces, not " + unsupported + "; choose an end face with up_to_face";
    return FC_OCCT_UNSUPPORTED;
  }
  if (forward) return FC_OCCT_OK;
  reason = "the target's next boundary does not close the profile without a new transition face; "
           "choose an end face with up_to_face";
  return FC_OCCT_INVALID_INPUT;
}
} // namespace next_boundary
