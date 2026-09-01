// SPDX-License-Identifier: MIT
// Independent, measurement-only FBX inspection through pinned ufbx.

#include "ufbx.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>

static void json_string_data(const char *data, size_t length)
{
    putchar('"');
    for (size_t i = 0; i < length; i++) {
        unsigned char ch = (unsigned char)data[i];
        switch (ch) {
        case '"': fputs("\\\"", stdout); break;
        case '\\': fputs("\\\\", stdout); break;
        case '\b': fputs("\\b", stdout); break;
        case '\f': fputs("\\f", stdout); break;
        case '\n': fputs("\\n", stdout); break;
        case '\r': fputs("\\r", stdout); break;
        case '\t': fputs("\\t", stdout); break;
        default:
            if (ch < 0x20) {
                printf("\\u%04x", (unsigned)ch);
            } else {
                putchar((int)ch);
            }
            break;
        }
    }
    putchar('"');
}

static void json_string(ufbx_string value)
{
    json_string_data(value.data, value.length);
}

static const char *basename_only(const char *path)
{
    const char *slash = strrchr(path, '/');
    const char *backslash = strrchr(path, '\\');
    const char *last = slash;
    if (!last || (backslash && backslash > last)) {
        last = backslash;
    }
    return last ? last + 1 : path;
}

static const char *axis_name(ufbx_coordinate_axis axis)
{
    switch (axis) {
    case UFBX_COORDINATE_AXIS_POSITIVE_X: return "+X";
    case UFBX_COORDINATE_AXIS_NEGATIVE_X: return "-X";
    case UFBX_COORDINATE_AXIS_POSITIVE_Y: return "+Y";
    case UFBX_COORDINATE_AXIS_NEGATIVE_Y: return "-Y";
    case UFBX_COORDINATE_AXIS_POSITIVE_Z: return "+Z";
    case UFBX_COORDINATE_AXIS_NEGATIVE_Z: return "-Z";
    default: return "unknown";
    }
}

static void vec3(ufbx_vec3 value)
{
    printf("[%.17g,%.17g,%.17g]", value.x, value.y, value.z);
}

static void quat(ufbx_quat value)
{
    printf("[%.17g,%.17g,%.17g,%.17g]", value.x, value.y, value.z, value.w);
}

static void print_user_properties(const ufbx_node *node)
{
    putchar('[');
    bool first = true;
    for (size_t i = 0; i < node->props.props.count; i++) {
        const ufbx_prop *prop = &node->props.props.data[i];
        if (!(prop->flags & UFBX_PROP_FLAG_USER_DEFINED)) continue;
        if (!first) putchar(',');
        first = false;
        fputs("{\"name\":", stdout);
        json_string(prop->name);
        printf(",\"type\":%u,\"int\":%" PRId64 ",\"string\":", (unsigned)prop->type, prop->value_int);
        json_string(prop->value_str);
        putchar('}');
    }
    putchar(']');
}

static void print_nodes(const ufbx_scene *scene)
{
    putchar('[');
    bool first = true;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->is_root) continue;
        if (!first) putchar(',');
        first = false;
        fputs("{\"order\":", stdout);
        printf("%zu,\"name\":", i - 1);
        json_string(node->name);
        fputs(",\"parent\":", stdout);
        if (!node->parent || node->parent->is_root) {
            fputs("\"<implicit-root>\"", stdout);
        } else {
            json_string(node->parent->name);
        }
        fputs(",\"translation\":", stdout);
        vec3(node->local_transform.translation);
        fputs(",\"euler_rotation_xyz_degrees\":", stdout);
        vec3(node->euler_rotation);
        fputs(",\"rotation_quaternion\":", stdout);
        quat(node->local_transform.rotation);
        fputs(",\"scale\":", stdout);
        vec3(node->local_transform.scale);
        fputs(",\"mesh\":", stdout);
        if (node->mesh) json_string(node->mesh->name); else fputs("null", stdout);
        fputs(",\"materials\":[", stdout);
        for (size_t material = 0; material < node->materials.count; material++) {
            if (material) putchar(',');
            json_string(node->materials.data[material]->name);
        }
        fputs("],\"user_properties\":", stdout);
        print_user_properties(node);
        putchar('}');
    }
    putchar(']');
}

static void print_mesh(const ufbx_mesh *mesh)
{
    fputs("{\"name\":", stdout);
    json_string(mesh->name);
    printf(",\"vertex_count\":%zu,\"polygon_vertex_count\":%zu,\"face_count\":%zu,\"triangle_count\":%zu",
        mesh->num_vertices, mesh->num_indices, mesh->num_faces, mesh->num_triangles);
    fputs(",\"instances\":[", stdout);
    for (size_t i = 0; i < mesh->instances.count; i++) {
        if (i) putchar(',');
        json_string(mesh->instances.data[i]->name);
    }
    fputs("],\"vertices\":[", stdout);
    for (size_t i = 0; i < mesh->vertices.count; i++) {
        if (i) putchar(',');
        vec3(mesh->vertices.data[i]);
    }
    fputs("],\"polygons\":[", stdout);
    for (size_t face_index = 0; face_index < mesh->faces.count; face_index++) {
        if (face_index) putchar(',');
        ufbx_face face = mesh->faces.data[face_index];
        fputs("{\"indices\":[", stdout);
        for (size_t corner = 0; corner < face.num_indices; corner++) {
            if (corner) putchar(',');
            printf("%u", mesh->vertex_indices.data[face.index_begin + corner]);
        }
        printf("],\"material\":%u}", mesh->face_material.data[face_index]);
    }
    fputs("],\"normals_by_polygon_vertex\":[", stdout);
    for (size_t i = 0; i < mesh->num_indices; i++) {
        if (i) putchar(',');
        vec3(ufbx_get_vertex_vec3(&mesh->vertex_normal, i));
    }
    fputs("],\"materials\":[", stdout);
    for (size_t i = 0; i < mesh->materials.count; i++) {
        if (i) putchar(',');
        json_string(mesh->materials.data[i]->name);
    }
    fputs("]}", stdout);
}

static void print_materials(const ufbx_scene *scene)
{
    putchar('[');
    for (size_t i = 0; i < scene->materials.count; i++) {
        if (i) putchar(',');
        const ufbx_material *material = scene->materials.data[i];
        fputs("{\"name\":", stdout);
        json_string(material->name);
        fputs(",\"diffuse_colour\":", stdout);
        vec3(material->fbx.diffuse_color.value_vec3);
        putchar('}');
    }
    putchar(']');
}

static int print_file(const char *path)
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
        fprintf(stderr, "%s: %s\n", basename_only(path), description);
        return 1;
    }

    fputs("{\"file\":", stdout);
    json_string_data(basename_only(path), strlen(basename_only(path)));
    printf(",\"format\":\"%s\",\"fbx_version\":%u,\"warnings\":%zu",
        scene->metadata.ascii ? "ascii" : "binary", scene->metadata.version,
        scene->metadata.warnings.count);
    fputs(",\"axes\":{\"right\":", stdout);
    json_string_data(axis_name(scene->settings.axes.right), strlen(axis_name(scene->settings.axes.right)));
    fputs(",\"up\":", stdout);
    json_string_data(axis_name(scene->settings.axes.up), strlen(axis_name(scene->settings.axes.up)));
    fputs(",\"front_opposite_forward\":", stdout);
    json_string_data(axis_name(scene->settings.axes.front), strlen(axis_name(scene->settings.axes.front)));
    printf("},\"unit_meters\":%.17g,\"node_count_excluding_implicit_root\":%zu,\"mesh_count\":%zu",
        scene->settings.unit_meters, scene->nodes.count - 1, scene->meshes.count);

    bool detailed = strstr(basename_only(path), "yup_m_preconverted_ascii7400") != NULL;
    if (detailed) {
        fputs(",\"nodes\":", stdout);
        print_nodes(scene);
        fputs(",\"mesh\":", stdout);
        if (scene->meshes.count == 1) print_mesh(scene->meshes.data[0]); else fputs("null", stdout);
        fputs(",\"materials\":", stdout);
        print_materials(scene);
    }
    putchar('}');
    ufbx_free_scene(scene);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: read_fixture FILE.fbx [FILE.fbx ...]\n");
        return 2;
    }

    printf("{\"schema\":\"ferritecad.independent-fbx-smoke.v1\",\"reader\":\"ufbx %u.%u.%u\",\"strict\":true,\"files\":[",
        ufbx_version_major(ufbx_source_version),
        ufbx_version_minor(ufbx_source_version),
        ufbx_version_patch(ufbx_source_version));
    for (int i = 1; i < argc; i++) {
        if (i > 1) putchar(',');
        if (print_file(argv[i]) != 0) return 1;
    }
    fputs("]}\n", stdout);
    return 0;
}
