# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Everything here is read by the scripts that source this file.
# shellcheck disable=SC2034
#
# Shared definitions for the Rust SBOM fragment scripts. Sourced, never run.
#
# The product roots, the features the release builds them with and the product
# targets are deliberately not defined here. They already have an owner in
# tools/notices/lib.sh, which the notices were the first to need; a second copy
# would let the SBOM describe a product the notices do not, and both files
# would go on looking right. This file adds only what is about the SBOM.

# shellcheck source=tools/notices/lib.sh
. tools/notices/lib.sh

# Where the generated fragments live. One file per product target: a fragment
# is a statement about one platform, and a file that covered several would have
# to say which parts applied where.
readonly SBOM_DIR='sbom/rust'

# The checked-in CycloneDX schema and its two companions.
readonly SBOM_SCHEMA_DIR='tools/sbom/schema'
readonly SBOM_SCHEMA_BOM="$SBOM_SCHEMA_DIR/bom-1.5.schema.json"
readonly SBOM_SCHEMA_SPDX="$SBOM_SCHEMA_DIR/spdx.schema.json"
readonly SBOM_SCHEMA_JSF="$SBOM_SCHEMA_DIR/jsf-0.82.schema.json"

# The closed inventory of packages whose publisher ships no licence text
# anywhere. ADR 0003 makes it a recorded risk and not a blocker, so the
# fragment marks those components with a property and nothing else. This is the
# notices' file, read here rather than copied.
readonly SBOM_RISK_TSV="$NOTICE_DECLARED_TSV"

# The property namespace. `cdx:` is CycloneDX's own; anything this project
# decides is under its own name so that a reader can tell which is which.
readonly SBOM_NS='ferritecad:sbom'

sbom_output_for() { # target
    printf '%s/rust-fragment-%s.cdx.json\n' "$SBOM_DIR" "$1"
}

sbom_die() {
    echo "${SBOM_TOOL:-sbom}: $*" >&2
    exit 1
}

# The pinned identities have exactly one owner, and every script reads them
# from there rather than carrying a copy.
sbom_load_pin() {
    # shellcheck source=tools/sbom/pin.env
    . tools/sbom/pin.env
    local v
    for v in CARGO_CYCLONEDX_VERSION JSONSCHEMA_CLI_VERSION CYCLONEDX_SPEC_VERSION \
             CYCLONEDX_SCHEMA_TAG CYCLONEDX_SCHEMA_COMMIT CYCLONEDX_SCHEMA_BOM_SHA256 \
             CYCLONEDX_SCHEMA_SPDX_SHA256 CYCLONEDX_SCHEMA_JSF_SHA256 \
             SBOM_FRAGMENT_FORMAT; do
        [ -n "${!v:-}" ] || sbom_die "tools/sbom/pin.env does not set $v"
    done
}

# A generator of the wrong version writes a file that is still a CycloneDX
# document, so the version is checked rather than assumed.
sbom_require_cyclonedx() {
    local found
    command -v cargo-cyclonedx >/dev/null 2>&1 || sbom_die \
        "cargo-cyclonedx is not installed; run: cargo install cargo-cyclonedx --version $CARGO_CYCLONEDX_VERSION --locked"
    found="$(cargo cyclonedx --version 2>/dev/null | awk '{print $NF}')"
    [ "$found" = "$CARGO_CYCLONEDX_VERSION" ] || sbom_die \
        "cargo-cyclonedx $found is installed but $CARGO_CYCLONEDX_VERSION is pinned; run: cargo install cargo-cyclonedx --version $CARGO_CYCLONEDX_VERSION --locked"
}

sbom_require_validator() {
    local found
    command -v jsonschema-cli >/dev/null 2>&1 || sbom_die \
        "jsonschema-cli is not installed; run: cargo install jsonschema-cli --version $JSONSCHEMA_CLI_VERSION --locked"
    found="$(jsonschema-cli --version 2>/dev/null | awk '{print $NF}')"
    [ "$found" = "$JSONSCHEMA_CLI_VERSION" ] || sbom_die \
        "jsonschema-cli $found is installed but $JSONSCHEMA_CLI_VERSION is pinned; run: cargo install jsonschema-cli --version $JSONSCHEMA_CLI_VERSION --locked"
}

sbom_require_jq() {
    command -v jq >/dev/null 2>&1 || sbom_die 'jq is not installed'
}

# The checked-in schema is the definition of valid, so a file that has been
# edited under it is a refusal rather than a different answer.
sbom_verify_schema_files() {
    local f d expected
    for f in "$SBOM_SCHEMA_BOM:$CYCLONEDX_SCHEMA_BOM_SHA256" \
             "$SBOM_SCHEMA_SPDX:$CYCLONEDX_SCHEMA_SPDX_SHA256" \
             "$SBOM_SCHEMA_JSF:$CYCLONEDX_SCHEMA_JSF_SHA256"; do
        expected="${f##*:}"; f="${f%%:*}"
        [ -f "$f" ] || sbom_die "$f is missing; it is part of the pinned schema"
        d="$(notice_sha256 "$f")"
        [ "$d" = "$expected" ] || sbom_die \
            "$f hashes to $d but tools/sbom/pin.env pins $expected"
    done
}

# jq writes CRLF on Windows, where its stdout is opened in text mode. A digest
# taken over those bytes is a different digest for a reason that has nothing to
# do with the dependency graph, so every jq result this project writes to a
# file goes through here. Carriage returns inside JSON strings are escaped as
# \r by jq itself and are not touched by this.
sbom_strip_cr() {
    tr -d '\r'
}
