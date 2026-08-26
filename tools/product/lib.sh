# SPDX-License-Identifier: MIT
# shellcheck shell=bash
# Everything here is read by the scripts that source this file.
# shellcheck disable=SC2034
#
# Shared definitions for the product SBOM scripts. Sourced, never run.
#
# The product SBOM is the merge of two intermediate documents that stay exactly
# as they were: the Rust CycloneDX fragment of §21A-2b2b0b2a and the native and
# assets inventory of §21A-2b2b0b2b1. Neither of those becomes a product SBOM
# retroactively; both go on saying they are incomplete, because they are, and
# the completed document is the third file this writes.
#
# Nothing about the product is defined here that another file already owns. The
# product roots, their features and the product targets belong to
# tools/notices/lib.sh; the fragment's identity and the pinned CycloneDX schema
# belong to tools/sbom; the native components, their relationships and the
# inventory's own format belong to tools/native.

# shellcheck source=tools/native/lib.sh
. tools/native/lib.sh
# shellcheck source=tools/sbom/lib.sh
. tools/sbom/lib.sh

# Where the merged documents live. One file per product target, for the reason
# the fragment has one: a document covering several would have to say which of
# its parts applied where, and the whole point of this slice is that a reader
# does not have to work that out.
readonly PRODUCT_DIR='sbom/product'

# The shape of the merged document, and it is deliberately not the fragment's
# `ferritecad:sbom:fragment-format`. That number describes an input this slice
# does not touch; bumping it would say the fragment changed when it did not.
# A consumer of the product SBOM reads this one.
readonly PRODUCT_FORMAT=1

# The workflow that proves the merge is the same on every host.
readonly PRODUCT_WORKFLOW='.github/workflows/product-sbom.yml'

product_output_for() { # target
    printf '%s/ferritecad-product-%s.cdx.json\n' "$PRODUCT_DIR" "$1"
}

product_die() {
    echo "${PRODUCT_TOOL:-product-sbom}: $*" >&2
    exit 1
}

# The two inputs, named once. A gate that looked for them under a second
# spelling could pass while merging something else.
product_fragment_for() { # target
    sbom_output_for "$1"
}
product_inventory() {
    printf '%s\n' "$NATIVE_INVENTORY"
}
