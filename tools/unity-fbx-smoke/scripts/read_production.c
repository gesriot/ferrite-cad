// SPDX-License-Identifier: MIT
// The independent §22B-1b2 gate: pinned ufbx reads what the production FBX
// writer produced, and says whether the measured contract survived.
//
// This is not the measurement fixture reader beside it. It never opens a
// committed fixture: it is given the bytes the Rust writer just wrote and
// checks them against the §22B-1a contract from the outside.

#include "ufbx.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

static int checks = 0;
static int failures = 0;

static void require(bool condition, const char *what)
{
    checks++;
    if (!condition) {
        failures++;
        fprintf(stderr, "FAIL %s\n", what);
    }
}

static void require_near(double actual, double expected, double tolerance, const char *what)
{
    checks++;
    if (!(fabs(actual - expected) <= tolerance)) {
        failures++;
        fprintf(stderr, "FAIL %s: %.17g is not %.17g within %g\n", what, actual, expected, tolerance);
    }
}

static bool name_is(ufbx_string value, const char *expected)
{
    size_t length = strlen(expected);
    return value.length == length && memcmp(value.data, expected, length) == 0;
}

static ufbx_node *find_node(ufbx_scene *scene, const char *name)
{
    for (size_t i = 0; i < scene->nodes.count; i++) {
        ufbx_node *node = scene->nodes.data[i];
        if (!node->is_root && name_is(node->name, name)) return node;
    }
    return NULL;
}

static const char *user_string(ufbx_node *node, const char *name)
{
    ufbx_prop *prop = ufbx_find_prop(&node->props, name);
    if (!prop) return NULL;
    if (!(prop->flags & UFBX_PROP_FLAG_USER_DEFINED)) return NULL;
    return prop->value_str.data;
}

// ------------------------------------------------------------ the contract

// (x, y, z) -> (x, z, -y): the one measured axis map, written out here rather
// than taken from the writer, so agreement means something.
static const double C[3][3] = {
    { 1.0, 0.0, 0.0 },
    { 0.0, 0.0, 1.0 },
    { 0.0, -1.0, 0.0 },
};

static void euler_xyz(const double degrees[3], double out[3][3])
{
    double x = degrees[0] * 3.14159265358979323846 / 180.0;
    double y = degrees[1] * 3.14159265358979323846 / 180.0;
    double z = degrees[2] * 3.14159265358979323846 / 180.0;
    double sx = sin(x), cx = cos(x);
    double sy = sin(y), cy = cos(y);
    double sz = sin(z), cz = cos(z);
    out[0][0] = cz * cy; out[0][1] = cz * sy * sx - sz * cx; out[0][2] = cz * sy * cx + sz * sx;
    out[1][0] = sz * cy; out[1][1] = sz * sy * sx + cz * cx; out[1][2] = sz * sy * cx - cz * sx;
    out[2][0] = -sy;     out[2][1] = cy * sx;                out[2][2] = cy * cx;
}

// C * m * C^T
static void conjugate(const double m[3][3], double out[3][3])
{
    double left[3][3];
    for (int r = 0; r < 3; r++) {
        for (int c = 0; c < 3; c++) {
            left[r][c] = C[r][0] * m[0][c] + C[r][1] * m[1][c] + C[r][2] * m[2][c];
        }
    }
    for (int r = 0; r < 3; r++) {
        for (int c = 0; c < 3; c++) {
            out[r][c] = left[r][0] * C[c][0] + left[r][1] * C[c][1] + left[r][2] * C[c][2];
        }
    }
}

static double element(const ufbx_matrix *matrix, int row, int column)
{
    return matrix->cols[column].v[row];
}

// ------------------------------------------------------------ the measured file

static const char *MEASURED_NODES[9] = {
    "Assembly Root", "Assembly Frame", "Repeated Part", "Repeated Part",
    "Omitted #2583", "CP Origin", "CP X1000", "CP Y2000", "CP Z3000",
};

static const char *MEASURED_PARENTS[9] = {
    "", "Assembly Root", "Assembly Frame", "Assembly Frame",
    "Assembly Frame", "Repeated Part", "Repeated Part", "Repeated Part", "Repeated Part",
};

static void check_measured(ufbx_scene *scene)
{
    require(scene->metadata.ascii, "the production file is ASCII");
    require(scene->metadata.version == 7400, "the production file is FBX 7400");
    require(scene->metadata.warnings.count == 0, "the reader accepted the file with no warning");

    require(scene->settings.axes.right == UFBX_COORDINATE_AXIS_POSITIVE_X, "right is +X");
    require(scene->settings.axes.up == UFBX_COORDINATE_AXIS_POSITIVE_Y, "up is +Y");
    require(scene->settings.axes.front == UFBX_COORDINATE_AXIS_POSITIVE_Z, "front-opposite-forward is +Z");
    require_near(scene->settings.unit_meters, 1.0, 1e-12, "the unit is one metre");

    require(scene->nodes.count - 1 == 9, "nine nodes, one model per scene node");
    require(scene->meshes.count == 1, "one geometry for one mesh definition");
    require(scene->materials.count == 4, "two definition slots and two overriding slots");

    // Hierarchy: exact names and exact parents, in file order.
    size_t seen = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (seen < 9) {
            require(name_is(node->name, MEASURED_NODES[seen]), "node order and name");
            bool root_child = node->parent == NULL || node->parent->is_root;
            if (MEASURED_PARENTS[seen][0] == '\0') {
                require(root_child, "the scene root sits at the file root");
            } else {
                require(!root_child && name_is(node->parent->name, MEASURED_PARENTS[seen]),
                    "node parent");
            }
        }
        seen++;
    }
    require(seen == 9, "every node was visited once");

    // One geometry, two placements of it.
    ufbx_mesh *mesh = scene->meshes.data[0];
    require(mesh->instances.count == 2, "one geometry is connected to both placements");
    if (mesh->instances.count == 2) {
        require(name_is(mesh->instances.data[0]->name, "Repeated Part")
            && name_is(mesh->instances.data[1]->name, "Repeated Part"),
            "both placements are the repeated part");
        require(mesh->instances.data[0] != mesh->instances.data[1],
            "two placements are two nodes");
    }

    // The converted geometry: 1000/2000/3000 mm along FerriteCAD X/Y/Z.
    static const double VERTICES[4][3] = {
        { 0.0, 0.0, 0.0 }, { 1.0, 0.0, 0.0 }, { 0.0, 0.0, -2.0 }, { 0.0, 3.0, 0.0 },
    };
    require(mesh->num_vertices == 4, "four control vertices");
    for (size_t i = 0; i < mesh->num_vertices && i < 4; i++) {
        require_near(mesh->vertices.data[i].x, VERTICES[i][0], 1e-12, "vertex x");
        require_near(mesh->vertices.data[i].y, VERTICES[i][1], 1e-12, "vertex y");
        require_near(mesh->vertices.data[i].z, VERTICES[i][2], 1e-12, "vertex z");
    }

    // The polygon order the writer must not reverse, and the slot each
    // polygon belongs to.
    static const uint32_t POLYGONS[4][3] = { {0,2,1}, {0,1,3}, {0,3,2}, {1,2,3} };
    static const uint32_t SLOTS[4] = { 0, 0, 1, 1 };
    require(mesh->num_faces == 4, "four polygons");
    for (size_t face = 0; face < mesh->faces.count && face < 4; face++) {
        require(mesh->faces.data[face].num_indices == 3, "a polygon is a triangle");
        for (size_t corner = 0; corner < 3; corner++) {
            uint32_t index = mesh->vertex_indices.data[mesh->faces.data[face].index_begin + corner];
            require(index == POLYGONS[face][corner], "polygon order");
        }
        require(mesh->face_material.data[face] == SLOTS[face], "polygon material slot");
    }

    // The authored normals, per polygon vertex, neither recalculated nor
    // averaged.
    static const double NORMALS[12][3] = {
        { 1, 0, 0 }, { 0, 1, 0 }, { 0, 0, -1 },
        { 1, 0, 0 }, { 0, 0, -1 }, { -1, 0, 0 },
        { 1, 0, 0 }, { -1, 0, 0 }, { 0, 1, 0 },
        { 0, 0, -1 }, { 0, 1, 0 }, { -1, 0, 0 },
    };
    require(mesh->num_indices == 12, "twelve polygon vertices");
    for (size_t i = 0; i < mesh->num_indices && i < 12; i++) {
        ufbx_vec3 normal = ufbx_get_vertex_vec3(&mesh->vertex_normal, i);
        require_near(normal.x, NORMALS[i][0], 1e-12, "authored normal x");
        require_near(normal.y, NORMALS[i][1], 1e-12, "authored normal y");
        require_near(normal.z, NORMALS[i][2], 1e-12, "authored normal z");
    }

    // Slots and the per-node override, which binds its own materials rather
    // than changing the shared definition.
    ufbx_node *plain = mesh->instances.count == 2 ? mesh->instances.data[0] : NULL;
    ufbx_node *recoloured = mesh->instances.count == 2 ? mesh->instances.data[1] : NULL;
    if (plain && recoloured) {
        require(plain->materials.count == 2 && recoloured->materials.count == 2,
            "two slots on each placement");
        require(plain->materials.data[0] != recoloured->materials.data[0],
            "the override did not bind its own material");
        ufbx_vec3 red = plain->materials.data[0]->fbx.diffuse_color.value_vec3;
        ufbx_vec3 blue = plain->materials.data[1]->fbx.diffuse_color.value_vec3;
        require_near(red.x, 0.8, 1e-4, "slot 0 colour r");
        require_near(red.y, 0.2, 1e-4, "slot 0 colour g");
        require_near(red.z, 0.1, 1e-4, "slot 0 colour b");
        require_near(blue.x, 0.1, 1e-4, "slot 1 colour r");
        require_near(blue.y, 0.35, 1e-4, "slot 1 colour g");
        require_near(blue.z, 0.9, 1e-4, "slot 1 colour b");
        ufbx_vec3 first = recoloured->materials.data[0]->fbx.diffuse_color.value_vec3;
        ufbx_vec3 second = recoloured->materials.data[1]->fbx.diffuse_color.value_vec3;
        require_near(first.x, second.x, 1e-12, "the override colours every slot alike");
        require_near(first.y, second.y, 1e-12, "the override colours every slot alike");
        require_near(first.z, second.z, 1e-12, "the override colours every slot alike");
        require(fabs(first.x - red.x) > 1e-3, "the override is a different colour");
    }

    // The local transform, conjugated once and decomposed in one declared
    // rotation order.
    ufbx_node *frame = find_node(scene, "Assembly Frame");
    require(frame != NULL, "the assembly frame survived");
    if (frame) {
        require(frame->rotation_order == UFBX_ROTATION_ORDER_XYZ,
            "the declared rotation order is XYZ");
        require_near(frame->local_transform.translation.x, 0.1, 1e-12, "frame local x");
        require_near(frame->local_transform.translation.y, 0.3, 1e-12, "frame local y");
        require_near(frame->local_transform.translation.z, -0.2, 1e-12, "frame local z");
        require_near(frame->local_transform.scale.x, 1.0, 1e-12, "no hidden root scale");
        require_near(frame->local_transform.scale.y, 1.0, 1e-12, "no hidden root scale");
        require_near(frame->local_transform.scale.z, 1.0, 1e-12, "no hidden root scale");

        static const double DEGREES[3] = { 11.0, 23.0, -17.0 };
        double source[3][3], expected[3][3];
        euler_xyz(DEGREES, source);
        conjugate(source, expected);
        for (int r = 0; r < 3; r++) {
            for (int c = 0; c < 3; c++) {
                require_near(element(&frame->node_to_parent, r, c), expected[r][c], 1e-9,
                    "the local rotation is the conjugated FerriteCAD rotation");
            }
        }
    }

    // The world matrix is the chain and not a second accumulation, and the
    // measured control distances are exactly 0, 1, 2 and 3 metres.
    ufbx_node *origin = find_node(scene, "CP Origin");
    require(origin != NULL, "the control points survived");
    if (origin) {
        static const char *POINTS[3] = { "CP X1000", "CP Y2000", "CP Z3000" };
        static const double DISTANCES[3] = { 1.0, 2.0, 3.0 };
        double ox = element(&origin->node_to_world, 0, 3);
        double oy = element(&origin->node_to_world, 1, 3);
        double oz = element(&origin->node_to_world, 2, 3);
        for (int i = 0; i < 3; i++) {
            ufbx_node *point = find_node(scene, POINTS[i]);
            require(point != NULL, "a control point survived");
            if (!point) continue;
            double dx = element(&point->node_to_world, 0, 3) - ox;
            double dy = element(&point->node_to_world, 1, 3) - oy;
            double dz = element(&point->node_to_world, 2, 3) - oz;
            require_near(sqrt(dx * dx + dy * dy + dz * dz), DISTANCES[i], 1e-9,
                "the control distance in world units");
        }
        // World is the product of the chain, computed here rather than read.
        ufbx_matrix product = ufbx_identity_matrix;
        ufbx_node *chain[8];
        size_t depth = 0;
        for (ufbx_node *walk = origin; walk && !walk->is_root && depth < 8; walk = walk->parent) {
            chain[depth++] = walk;
        }
        for (size_t i = depth; i > 0; i--) {
            product = ufbx_matrix_mul(&product, &chain[i - 1]->node_to_parent);
        }
        for (int r = 0; r < 3; r++) {
            for (int c = 0; c < 4; c++) {
                require_near(element(&product, r, c), element(&origin->node_to_world, r, c), 1e-9,
                    "the world matrix is the product of the local chain");
            }
        }
    }

    // The omission, as properties an importer callback can read.
    ufbx_node *omitted = find_node(scene, "Omitted #2583");
    require(omitted != NULL, "the omitted definition kept its hierarchy node");
    if (omitted) {
        require(omitted->mesh == NULL, "the omitted definition was given no triangles");
        const char *key = user_string(omitted, "FerriteCADGeometryOmission");
        require(key && strcmp(key, "step.product_definition#2583") == 0,
            "FerriteCADGeometryOmission names the source-local definition");
        const char *definition = user_string(omitted, "FerriteCADDefinitionKey");
        require(definition && strcmp(definition, "step.product_definition#2583") == 0,
            "FerriteCADDefinitionKey names the source-local definition");
        const char *finding = user_string(omitted, "FerriteCADOmissionFinding");
        require(finding && strcmp(finding, "step.product_definition#2583") == 0,
            "FerriteCADOmissionFinding names the persisted finding entity");
        const char *refusal = user_string(omitted, "FerriteCADOmissionRefusal");
        require(refusal && strcmp(refusal, "IncompleteFace") == 0,
            "FerriteCADOmissionRefusal is the stable typed name");
        require(user_string(omitted, "FerriteCADNodeKey") != NULL, "the omitted node has a key");
    }

    // Structure is not an omission.
    static const char *STRUCTURAL[3] = { "Assembly Root", "Assembly Frame", "CP Origin" };
    for (int i = 0; i < 3; i++) {
        ufbx_node *node = find_node(scene, STRUCTURAL[i]);
        require(node != NULL, "a structural node survived");
        if (!node) continue;
        require(node->mesh == NULL, "a structural node has no geometry");
        require(user_string(node, "FerriteCADGeometryOmission") == NULL,
            "a structural node was marked as a missing part");
        require(user_string(node, "FerriteCADNodeKey") != NULL,
            "a structural node has a key");
    }
}

// ------------------------------------------------------------ the escaping file

static const char *ESCAPED_NAMES[5] = {
    "a \"quoted\" name",
    "back\\slash and\ttab",
    ("\xd0\x9a\xd0\xb8\xd1\x80\xd0\xb8\xd0\xbb\xd0\xbb\xd0\xb8\xd1\x86\xd0\xb0 \xd0\xb8 "
     "\xd1\x8e\xd0\xbd\xd0\xb8\xd0\xba\xd0\xbe\xd0\xb4 \xe2\x80\x94 ok"),
    "",
    "line\nbreak and\rreturn",
};

static void check_escaping(ufbx_scene *scene)
{
    require(scene->metadata.ascii, "the escaping file is ASCII");
    require(scene->metadata.version == 7400, "the escaping file is FBX 7400");
    require(scene->metadata.warnings.count == 0, "the escaping file read with no warning");
    require(scene->nodes.count - 1 == 5, "five named nodes");

    size_t seen = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (seen < 5) {
            require(name_is(node->name, ESCAPED_NAMES[seen]), "an escaped name survived exactly");
            if (!name_is(node->name, ESCAPED_NAMES[seen])) {
                fprintf(stderr, "  read [%.*s]\n", (int)node->name.length, node->name.data);
            }
        }
        seen++;
    }
    require(seen == 5, "every named node was visited");
}

// ------------------------------------------------------------ the complex assembly

// The real imported assembly: 46 definitions, 140 nodes, one root, 34
// geometries rather than the 112 draws a flattened picture of it has, and one
// typed omission that keeps its placements.
#define COMPLEX_REAL "step.product_definition#2428"
#define COMPLEX_OMITTED "step.product_definition#2583"

static void check_complex(ufbx_scene *scene)
{
    require(scene->metadata.ascii, "the complex file is ASCII");
    require(scene->metadata.version == 7400, "the complex file is FBX 7400");
    require(scene->metadata.warnings.count == 0, "the complex file read with no warning");
    require(scene->settings.axes.right == UFBX_COORDINATE_AXIS_POSITIVE_X, "complex right is +X");
    require(scene->settings.axes.up == UFBX_COORDINATE_AXIS_POSITIVE_Y, "complex up is +Y");
    require(scene->settings.axes.front == UFBX_COORDINATE_AXIS_POSITIVE_Z, "complex front is +Z");
    require_near(scene->settings.unit_meters, 1.0, 1e-12, "the complex unit is one metre");

    require(scene->nodes.count - 1 == 140, "one model per scene node");
    require(scene->meshes.count == 34, "one geometry per meshed definition, not one per draw");

    size_t roots = 0, marked = 0, omitted_nodes = 0, real_nodes = 0, definitions = 0;
    ufbx_mesh *shared = NULL;
    bool shared_set = false;
    // The definition keys already seen, so the count is of distinct ones.
    const ufbx_node *first_of[64];
    ufbx_string keys[64];

    for (size_t i = 0; i < scene->nodes.count; i++) {
        ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (!node->parent || node->parent->is_root) roots++;

        const char *key = user_string(node, "FerriteCADDefinitionKey");
        if (!key) {
            require(false, "a node has no definition key");
            continue;
        }
        require(user_string(node, "FerriteCADNodeKey") != NULL, "a node has no node key");

        bool known = false;
        for (size_t seen = 0; seen < definitions; seen++) {
            if (strcmp(keys[seen].data, key) == 0) { known = true; break; }
        }
        if (!known && definitions < 64) {
            keys[definitions].data = key;
            keys[definitions].length = strlen(key);
            first_of[definitions] = node;
            definitions++;
        } else if (!known) {
            definitions++;
        }

        const char *omission = user_string(node, "FerriteCADGeometryOmission");
        if (omission) {
            marked++;
            require(strcmp(omission, key) == 0, "an omission names its own definition");
            require(node->mesh == NULL, "an omitted node was given triangles");
            const char *refusal = user_string(node, "FerriteCADOmissionRefusal");
            require(refusal && strcmp(refusal, "IncompleteFace") == 0,
                "the omission carries the stable typed refusal name");
            require(user_string(node, "FerriteCADOmissionFinding") != NULL,
                "the omission carries the persisted finding entity");
        }

        if (strcmp(key, COMPLEX_OMITTED) == 0) {
            omitted_nodes++;
            require(omission != NULL, "#2583 lost its omission marker");
        } else if (strcmp(key, COMPLEX_REAL) == 0) {
            real_nodes++;
            require(node->mesh != NULL, "#2428 lost its geometry");
            require(omission == NULL, "#2428 was reported as missing");
            if (!shared_set) { shared = node->mesh; shared_set = true; }
            require(node->mesh == shared, "#2428's placements do not share one geometry");
        }
    }

    require(roots == 1, "the one root changed");
    require(definitions == 46, "a definition stopped being represented");
    require(real_nodes > 0, "#2428 left the assembly");
    require(omitted_nodes > 0, "#2583 lost its placements");
    require(marked == omitted_nodes, "a structural frame was marked as a missing part");
    require(shared_set && shared->num_faces > 0, "#2428 was healed away into an empty geometry");

    size_t triangles = 0;
    for (size_t i = 0; i < scene->meshes.count; i++) {
        ufbx_mesh *mesh = scene->meshes.data[i];
        require(mesh->num_faces > 0, "a geometry has no polygons");
        require(mesh->instances.count > 0, "a geometry nothing places was written");
        triangles += mesh->num_triangles;
    }
    require(triangles > 0, "the assembly has no triangles at all");
    printf("complex: nodes=140 geometries=%zu definitions=%zu triangles=%zu placements_of_2428=%zu\n",
        scene->meshes.count, definitions, triangles, real_nodes);
}

// ------------------------------------------------------------ driver

static ufbx_scene *load(const char *path)
{
    ufbx_load_opts opts = { 0 };
    opts.ignore_animation = true;
    opts.ignore_embedded = true;
    opts.evaluate_skinning = false;
    opts.evaluate_caches = false;
    opts.load_external_files = false;
    opts.generate_missing_normals = false;
    opts.strict = true;

    ufbx_error error;
    ufbx_scene *scene = ufbx_load_file(path, &opts, &error);
    if (!scene) {
        char description[4096];
        ufbx_format_error(description, sizeof(description), &error);
        fprintf(stderr, "FAIL the reader refused %s: %s\n", path, description);
        failures++;
        checks++;
    }
    return scene;
}

int main(int argc, char **argv)
{
    bool complex_mode = argc == 3 && strcmp(argv[1], "--complex") == 0;
    if (argc != 3 || (!complex_mode && argv[1][0] == '-')) {
        fprintf(stderr, "usage: read_production MEASURED.fbx ESCAPING.fbx\n");
        fprintf(stderr, "       read_production --complex COMPLEX.fbx\n");
        return 2;
    }

    printf("reader ufbx %u.%u.%u strict\n",
        ufbx_version_major(ufbx_source_version),
        ufbx_version_minor(ufbx_source_version),
        ufbx_version_patch(ufbx_source_version));

    int minimum = 100;
    if (complex_mode) {
        minimum = 250;
        ufbx_scene *scene = load(argv[2]);
        if (scene) {
            check_complex(scene);
            ufbx_free_scene(scene);
        }
    } else {
        ufbx_scene *measured = load(argv[1]);
        if (measured) {
            check_measured(measured);
            ufbx_free_scene(measured);
        }
        ufbx_scene *escaping = load(argv[2]);
        if (escaping) {
            check_escaping(escaping);
            ufbx_free_scene(escaping);
        }
    }

    if (checks < minimum) {
        fprintf(stderr, "FAIL the gate performed only %d checks\n", checks);
        failures++;
    }
    printf("FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=%d failures=%d\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
