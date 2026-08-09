// SPDX-License-Identifier: MIT
//
// FerriteCAD's own code. planegcs stays behind this boundary and behind a
// shared library that can be replaced; nothing of its API crosses into Rust.

#include "planegcs_shim.h"

#include <exception>
#include <vector>

#include "GCS.h"

namespace {

/// The points, held as planegcs wants them: pointers into one parameter block.
struct Sketch {
  std::vector<double *> parameters;
  std::vector<GCS::Point> points;
  // Constraint values planegcs takes by pointer, kept alive for the solve.
  std::vector<double> values;
};

GCS::Line line(Sketch &sketch, int32_t a, int32_t b) {
  GCS::Line result;
  result.p1 = sketch.points[static_cast<size_t>(a)];
  result.p2 = sketch.points[static_cast<size_t>(b)];
  return result;
}

bool is_point(size_t point_count, int32_t index) {
  return index >= 0 && static_cast<size_t>(index) < point_count;
}

bool has_valid_points(const FcGcsConstraint &constraint,
                      size_t point_count) {
  int used = 0;
  switch (constraint.kind) {
    case FC_GCS_FIXED:
      used = 1;
      break;
    case FC_GCS_COINCIDENT:
    case FC_GCS_DISTANCE:
    case FC_GCS_HORIZONTAL:
    case FC_GCS_VERTICAL:
      used = 2;
      break;
    case FC_GCS_EQUAL_LENGTH:
    case FC_GCS_PERPENDICULAR:
    case FC_GCS_PARALLEL:
      used = 4;
      break;
    default:
      return false;
  }
  for (int i = 0; i < used; ++i) {
    if (!is_point(point_count, constraint.points[i])) {
      return false;
    }
  }
  return true;
}

}  // namespace

extern "C" int32_t fc_gcs_solve(double *state, size_t point_count,
                                const FcGcsConstraint *constraints,
                                size_t constraint_count, int32_t *out_dofs,
                                int32_t *out_has_conflicting,
                                int32_t *out_has_redundant,
                                int32_t *out_iterations) noexcept {
  if (state == nullptr || (constraint_count > 0 && constraints == nullptr)) {
    return FC_GCS_INVALID_INPUT;
  }

  try {
    Sketch sketch;
    sketch.points.reserve(point_count);
    sketch.parameters.reserve(point_count * 2);
    // Reserved once: planegcs holds pointers into this, and a reallocation
    // would leave it reading freed memory.
    sketch.values.reserve(constraint_count * 2);

    for (size_t i = 0; i < point_count; ++i) {
      GCS::Point point;
      point.x = &state[i * 2];
      point.y = &state[i * 2 + 1];
      sketch.points.push_back(point);
      sketch.parameters.push_back(point.x);
      sketch.parameters.push_back(point.y);
    }

    GCS::System system;
    for (size_t i = 0; i < constraint_count; ++i) {
      const FcGcsConstraint &c = constraints[i];
      if (!has_valid_points(c, point_count)) {
        return c.kind < FC_GCS_COINCIDENT || c.kind > FC_GCS_PARALLEL
                   ? FC_GCS_UNKNOWN_CONSTRAINT
                   : FC_GCS_INVALID_INPUT;
      }
      const int tag = static_cast<int>(i) + 1;

      switch (c.kind) {
        case FC_GCS_COINCIDENT:
          system.addConstraintP2PCoincident(sketch.points[c.points[0]],
                                            sketch.points[c.points[1]], tag);
          break;
        case FC_GCS_FIXED: {
          sketch.values.push_back(c.value);
          double *x = &sketch.values.back();
          sketch.values.push_back(c.value2);
          double *y = &sketch.values.back();
          system.addConstraintCoordinateX(sketch.points[c.points[0]], x, tag);
          system.addConstraintCoordinateY(sketch.points[c.points[0]], y, tag);
          break;
        }
        case FC_GCS_DISTANCE: {
          sketch.values.push_back(c.value);
          system.addConstraintP2PDistance(sketch.points[c.points[0]],
                                          sketch.points[c.points[1]],
                                          &sketch.values.back(), tag);
          break;
        }
        case FC_GCS_HORIZONTAL:
          system.addConstraintHorizontal(sketch.points[c.points[0]],
                                         sketch.points[c.points[1]], tag);
          break;
        case FC_GCS_VERTICAL:
          system.addConstraintVertical(sketch.points[c.points[0]],
                                       sketch.points[c.points[1]], tag);
          break;
        case FC_GCS_EQUAL_LENGTH: {
          GCS::Line a = line(sketch, c.points[0], c.points[1]);
          GCS::Line b = line(sketch, c.points[2], c.points[3]);
          system.addConstraintEqualLength(a, b, tag);
          break;
        }
        case FC_GCS_PERPENDICULAR:
          system.addConstraintPerpendicular(
              sketch.points[c.points[0]], sketch.points[c.points[1]],
              sketch.points[c.points[2]], sketch.points[c.points[3]], tag);
          break;
        case FC_GCS_PARALLEL: {
          GCS::Line a = line(sketch, c.points[0], c.points[1]);
          GCS::Line b = line(sketch, c.points[2], c.points[3]);
          system.addConstraintParallel(a, b, tag);
          break;
        }
        default:
          return FC_GCS_UNKNOWN_CONSTRAINT;
      }
    }

    system.declareUnknowns(sketch.parameters);
    system.initSolution();

    // Asked before solving, which is when a person wants to be told that
    // their sketch is over-constrained.
    system.diagnose();
    if (out_dofs != nullptr) {
      *out_dofs = static_cast<int32_t>(system.dofsNumber());
    }
    if (out_has_conflicting != nullptr) {
      *out_has_conflicting = system.hasConflicting() ? 1 : 0;
    }
    if (out_has_redundant != nullptr) {
      *out_has_redundant = system.hasRedundant() ? 1 : 0;
    }

    const int status = system.solve();
    if (out_iterations != nullptr) {
      // planegcs does not report an iteration count through this path.
      *out_iterations = -1;
    }

    // Success zeroes the error function; Converged minimises it, which is the
    // honest answer for a system that cannot be satisfied exactly. Both are
    // solutions and both must be written back — an earlier version of this
    // shim returned on Converged without applying, and every sketch with a
    // redundant constraint came back untouched and looked like a solver
    // failure. Only Failed leaves the state alone.
    if (status != GCS::Success && status != GCS::Converged) {
      return FC_GCS_NOT_CONVERGED;
    }
    system.applySolution();
    return status == GCS::Success ? FC_GCS_SUCCESS : FC_GCS_CONVERGED;
  } catch (const std::exception &) {
    return FC_GCS_STD_EXCEPTION;
  } catch (...) {
    return FC_GCS_UNKNOWN_EXCEPTION;
  }
}

extern "C" const char *fc_gcs_provenance(void) noexcept {
  return "planegcs from FreeCAD 1.0.1";
}
