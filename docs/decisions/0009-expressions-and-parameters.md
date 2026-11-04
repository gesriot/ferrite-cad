# 9. Expressions and parameters

**Status:** accepted design.

Model dimensions already store source text and a value in internal units. To let one
parameter drive several features, preserve the user's spelling and make the references and
the computed value verifiable. One pure Rust implementation in `ferritecad-document` serves
transactions, rebuilds and the CLI.

## Language and units

Use decimal literals with an optional decimal point and decimal exponent. A literal may
have a case-sensitive unit suffix, optionally separated by whitespace. Convert `mm`, `cm`,
`m`, `in`, `ft`, `deg` and `rad` through `Quantity::base_value`, as for numeric edits.
A lone number, including a negative number, uses the slot's base unit. Inside an expression
a number without a unit is scalar. For a length `h`, both `2 * h` and `h + 5 mm` are
lengths. Adding `5` to `h` is a dimension error.

Parentheses group terms. Unary minus binds tightest, then multiplication and division,
then addition and subtraction. Binary operators at the same precedence associate left to
right. Preserve evaluation order without fused multiply-add or reassociation. Reject
functions, powers, division by zero and non-finite results.

Track integer exponents of length and angle. Multiplication adds exponents and division
subtracts them. Addition and subtraction require equal dimensions. Intermediate areas and
ratios are valid. The final dimension must match the slot or parameter declaration.

## Graph and persistence

A parameter has a unique case-sensitive name matching `[A-Za-z_][A-Za-z0-9_]*`, a declared
dimension and an expression. Reserve the seven unit names. Resolve source tokens by name
and store one `parameter` dependency from the owning object to each referenced parameter ID.
Derive those rows on edit. On open, a missing or extra row is a structural `input` error.
Unparseable source text and an unknown parameter name are also structural `input` errors on open.
Reject cycles and identify the parameters in the cycle by name.

Evaluate the edit candidate once in graph order and store every affected value in the same
transaction and Undo step. Renaming rewrites only matching identifier tokens, preserving
all other source bytes. Removing a referenced parameter refuses and lists its dependents.
Cold and cached rebuilds evaluate in graph order and compare stored and computed value bits
before using a geometry result. Geometry cache keys use the value, so equivalent spelling
and a parameter rename can reuse the result.

Report expression errors against the object and slot, including the name for a parameter
and the source position for a syntax error. A failed parameter or mismatched stored value
follows the per-object status rules in [ADR 0006](0006-generic-modelling-core.md).
Identify the failed parameter to its dependents and continue rebuilding independent objects.

The CLI accepts a text expression or a numeric value and unit for a slot. Window forms edit
only a single numeric literal with no parameter reference and direct other edits to `edit`.
