// SPDX-License-Identifier: MIT
//
// FerriteCAD's own code. planegcs stays behind this boundary and behind a
// shared library that can be replaced; nothing of its API crosses into Rust.

#include "planegcs_shim.h"

#include <cmath>
#include <cstdint>
#include <cstring>
#include <exception>
#include <memory>
#include <vector>

#include "GCS.h"

namespace {

/// A line between two of a sketch's points, by index.
GCS::Line line_of(const std::vector<GCS::Point> &points, int32_t a, int32_t b) {
  GCS::Line result;
  result.p1 = points[static_cast<size_t>(a)];
  result.p2 = points[static_cast<size_t>(b)];
  return result;
}

// Counted where the crossing happens, so the count is of crossings and not of
// intentions. See the header for why these exist and why they are per thread.
thread_local uint64_t native_solves = 0;
thread_local uint64_t native_sessions = 0;
// Incremented on creation and decremented on destruction, so a session that
// was never released and one that was released twice are both visible from
// outside. Neither shows up in a coordinate.
thread_local uint64_t native_live_sessions = 0;

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

namespace {

/// A system that outlives one solve, so dragging can be measured honestly.
struct Session {
  std::vector<double> state;
  std::vector<double *> parameters;
  std::vector<GCS::Point> points;
  // Held by pointer inside planegcs, so this must never reallocate and the
  // slots must stay put for the session's whole life.
  std::vector<double> values;
  // Where each Fixed constraint's x target sits in `values`, or -1. Distance
  // values also live in the vector but must never be mistaken for drag targets.
  std::vector<long> fixed_value_of;
  GCS::System system;
  bool diagnosed = false;
  bool prepared = false;
};

}  // namespace

extern "C" FcGcsSession *fc_gcs_session_create(
    const double *start, size_t point_count, const FcGcsConstraint *constraints,
    size_t constraint_count) noexcept {
  if ((point_count > 0 && start == nullptr) ||
      (constraint_count > 0 && constraints == nullptr)) {
    return nullptr;
  }
  try {
    auto session = std::make_unique<Session>();
    // Rust's empty Vec has a non-null, aligned dangling pointer. It is legal
    // to pass as an empty slice but is not a C++ array iterator, so do not do
    // pointer arithmetic on it when the sketch has no points.
    if (point_count > 0) {
      session->state.assign(start, start + point_count * 2);
    }
    session->points.reserve(point_count);
    session->parameters.reserve(point_count * 2);
    session->values.reserve(constraint_count * 2);
    session->fixed_value_of.assign(constraint_count, -1);

    for (size_t i = 0; i < point_count; ++i) {
      GCS::Point point;
      point.x = &session->state[i * 2];
      point.y = &session->state[i * 2 + 1];
      session->points.push_back(point);
      session->parameters.push_back(point.x);
      session->parameters.push_back(point.y);
    }

    for (size_t i = 0; i < constraint_count; ++i) {
      const FcGcsConstraint &c = constraints[i];
      const int tag = static_cast<int>(i) + 1;
      if (!has_valid_points(c, point_count)) {
        return nullptr;
      }

      switch (c.kind) {
        case FC_GCS_COINCIDENT:
          session->system.addConstraintP2PCoincident(
              session->points[c.points[0]], session->points[c.points[1]], tag);
          break;
        case FC_GCS_FIXED: {
          session->fixed_value_of[i] =
              static_cast<long>(session->values.size());
          session->values.push_back(c.value);
          session->values.push_back(c.value2);
          session->system.addConstraintCoordinateX(
              session->points[c.points[0]],
              &session->values[static_cast<size_t>(
                  session->fixed_value_of[i])], tag);
          session->system.addConstraintCoordinateY(
              session->points[c.points[0]],
              &session->values[static_cast<size_t>(
                                   session->fixed_value_of[i]) +
                               1],
              tag);
          break;
        }
        case FC_GCS_DISTANCE: {
          const size_t slot = session->values.size();
          session->values.push_back(c.value);
          session->system.addConstraintP2PDistance(
              session->points[c.points[0]], session->points[c.points[1]],
              &session->values[slot], tag);
          break;
        }
        case FC_GCS_HORIZONTAL:
          session->system.addConstraintHorizontal(
              session->points[c.points[0]], session->points[c.points[1]], tag);
          break;
        case FC_GCS_VERTICAL:
          session->system.addConstraintVertical(
              session->points[c.points[0]], session->points[c.points[1]], tag);
          break;
        case FC_GCS_EQUAL_LENGTH: {
          GCS::Line a = line_of(session->points, c.points[0], c.points[1]);
          GCS::Line b = line_of(session->points, c.points[2], c.points[3]);
          session->system.addConstraintEqualLength(a, b, tag);
          break;
        }
        case FC_GCS_PERPENDICULAR:
          session->system.addConstraintPerpendicular(
              session->points[c.points[0]], session->points[c.points[1]],
              session->points[c.points[2]], session->points[c.points[3]], tag);
          break;
        case FC_GCS_PARALLEL: {
          GCS::Line a = line_of(session->points, c.points[0], c.points[1]);
          GCS::Line b = line_of(session->points, c.points[2], c.points[3]);
          session->system.addConstraintParallel(a, b, tag);
          break;
        }
        default:
          return nullptr;
      }
    }

    session->system.declareUnknowns(session->parameters);
    // Count systems that were actually built, not failed attempts that
    // returned null before a caller could own them.
    ++native_sessions;
    ++native_live_sessions;
    return reinterpret_cast<FcGcsSession *>(session.release());
  } catch (...) {
    return nullptr;
  }
}

extern "C" void fc_gcs_session_destroy(FcGcsSession *session) noexcept {
  if (session == nullptr) {
    return;
  }
  --native_live_sessions;
  delete reinterpret_cast<Session *>(session);
}

extern "C" int32_t fc_gcs_session_diagnose(
    FcGcsSession *handle, int32_t *out_dofs, int32_t *out_conflicting_count,
    int32_t *out_redundant_count, int32_t *out_blamed, size_t capacity,
    size_t *out_blamed_count) noexcept {
  if (handle == nullptr) {
    return FC_GCS_INVALID_INPUT;
  }
  try {
    Session *session = reinterpret_cast<Session *>(handle);
    if (capacity > 0 && out_blamed == nullptr) {
      return FC_GCS_INVALID_INPUT;
    }
    if (!session->diagnosed) {
      // The return value is the degree-of-freedom count, and it is allowed to
      // be negative: planegcs computes it as parameters minus non-redundant
      // constraints, so an over-constrained sketch reports fewer than zero.
      // Reading that as a failure - which this shim used to do - threw away
      // the conflicting tags for exactly the sketches whose conflict a person
      // most needs named, and reported them as a solver that would not answer.
      //
      // A diagnosis is unusable only when no unknowns were declared, and
      // fc_gcs_session_create always declares them before this can be called,
      // so there is no such case to distinguish here.
      session->system.diagnose();
      session->diagnosed = true;
    }

    GCS::VEC_I conflicting;
    GCS::VEC_I redundant;
    session->system.getConflicting(conflicting);
    session->system.getRedundant(redundant);

    if (out_dofs != nullptr) {
      *out_dofs = static_cast<int32_t>(session->system.dofsNumber());
    }
    if (out_conflicting_count != nullptr) {
      *out_conflicting_count = static_cast<int32_t>(conflicting.size());
    }
    if (out_redundant_count != nullptr) {
      *out_redundant_count = static_cast<int32_t>(redundant.size());
    }

    // Tags were set to the constraint's own index plus one, so what comes back
    // names the constraint a person wrote rather than a row of a matrix.
    size_t written = 0;
    size_t total = 0;
    for (const GCS::VEC_I *group : {&conflicting, &redundant}) {
      for (int tag : *group) {
        ++total;
        if (out_blamed != nullptr && written < capacity) {
          out_blamed[written++] = tag - 1;
        }
      }
    }
    if (out_blamed_count != nullptr) {
      *out_blamed_count = total;
    }
    return FC_GCS_SUCCESS;
  } catch (const std::exception &) {
    return FC_GCS_STD_EXCEPTION;
  } catch (...) {
    return FC_GCS_UNKNOWN_EXCEPTION;
  }
}

extern "C" int32_t fc_gcs_session_prepare(FcGcsSession *handle) noexcept {
  if (handle == nullptr) {
    return FC_GCS_INVALID_INPUT;
  }
  try {
    Session *session = reinterpret_cast<Session *>(handle);
    if (!session->diagnosed || session->prepared) {
      return FC_GCS_INVALID_INPUT;
    }
    // diagnose() has already populated the rank information, so
    // initSolution() partitions the system without running it a second time.
    session->system.initSolution();
    session->prepared = true;
    return FC_GCS_SUCCESS;
  } catch (const std::exception &) {
    return FC_GCS_STD_EXCEPTION;
  } catch (...) {
    return FC_GCS_UNKNOWN_EXCEPTION;
  }
}

extern "C" int32_t fc_gcs_session_move(FcGcsSession *handle,
                                       size_t constraint_index, double x,
                                       double y) noexcept {
  if (handle == nullptr || !std::isfinite(x) || !std::isfinite(y)) {
    return FC_GCS_INVALID_INPUT;
  }
  Session *session = reinterpret_cast<Session *>(handle);
  if (!session->prepared ||
      constraint_index >= session->fixed_value_of.size()) {
    return FC_GCS_INVALID_INPUT;
  }
  const long slot = session->fixed_value_of[constraint_index];
  if (slot < 0 || static_cast<size_t>(slot) + 1 >= session->values.size()) {
    return FC_GCS_INVALID_INPUT;
  }
  // Written through the pointers planegcs already holds, which is what makes
  // this a nudge rather than a rebuild.
  session->values[static_cast<size_t>(slot)] = x;
  session->values[static_cast<size_t>(slot) + 1] = y;
  return FC_GCS_SUCCESS;
}

extern "C" int32_t fc_gcs_session_solve(FcGcsSession *handle) noexcept {
  if (handle == nullptr) {
    return FC_GCS_INVALID_INPUT;
  }
  try {
    Session *session = reinterpret_cast<Session *>(handle);
    if (!session->prepared) {
      return FC_GCS_INVALID_INPUT;
    }
    ++native_solves;
    const int status = session->system.solve();
    if (status == GCS::Failed || status == GCS::SuccessfulSolutionInvalid) {
      return FC_GCS_NOT_CONVERGED;
    }
    session->system.applySolution();
    return status == GCS::Success ? FC_GCS_SUCCESS : FC_GCS_CONVERGED;
  } catch (const std::exception &) {
    return FC_GCS_STD_EXCEPTION;
  } catch (...) {
    return FC_GCS_UNKNOWN_EXCEPTION;
  }
}

extern "C" int32_t fc_gcs_session_state(const FcGcsSession *handle, double *out,
                                        size_t count) noexcept {
  if (handle == nullptr || (count > 0 && out == nullptr)) {
    return FC_GCS_INVALID_INPUT;
  }
  const Session *session = reinterpret_cast<const Session *>(handle);
  if (count != session->state.size()) {
    return FC_GCS_INVALID_INPUT;
  }
  // An empty vector may return null from data(). No bytes need copying, so do
  // not make the zero-point contract depend on a C library accepting a null
  // pointer for a zero-length memcpy.
  if (count > 0) {
    std::memcpy(out, session->state.data(), count * sizeof(double));
  }
  return FC_GCS_SUCCESS;
}

// Answered by the shared library, not by this shim.
//
// A string compiled in here would go on saying "FreeCAD 1.0.1" beside a
// library built from anything at all, which is exactly the substitution the
// packaging gate exists to catch. Asking the library means the answer arrives
// through the same dynamic link everything else does, and a library that
// cannot answer does not load.
extern "C" const char *fc_planegcs_provenance(void);

extern "C" const char *fc_gcs_provenance(void) noexcept {
  return fc_planegcs_provenance();
}

extern "C" uint64_t fc_gcs_native_solves(void) noexcept { return native_solves; }

extern "C" uint64_t fc_gcs_native_sessions(void) noexcept {
  return native_sessions;
}

extern "C" uint64_t fc_gcs_native_live_sessions(void) noexcept {
  return native_live_sessions;
}
