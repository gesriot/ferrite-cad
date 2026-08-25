# SPDX-License-Identifier: MIT
#
# Cargo.lock -> "name<TAB>version<TAB>source<TAB>checksum", one package a line.
#
# Cargo.lock is the owner of every package's exact source and digest, and both
# the fragment and the gate read them from here rather than from any tool's
# idea of them. A workspace member has neither a source nor a checksum and
# leaves both fields empty, which is the fact and not a gap.
#
# The format is a flat sequence of [[package]] tables whose scalar values are
# quoted strings on their own lines, so it is read without a TOML parser.

function flush() {
    if (name != "") printf "%s\t%s\t%s\t%s\n", name, version, source, checksum
    name = ""; version = ""; source = ""; checksum = ""
}

/^\[\[package\]\]$/ { flush(); in_package = 1; next }
/^\[/               { flush(); in_package = 0; next }

in_package && /^name = "/     { name     = unquote($0); next }
in_package && /^version = "/  { version  = unquote($0); next }
in_package && /^source = "/   { source   = unquote($0); next }
in_package && /^checksum = "/ { checksum = unquote($0); next }

END { flush() }

function unquote(line,   value) {
    value = line
    sub(/^[a-z]+ = "/, "", value)
    sub(/"$/, "", value)
    return value
}
