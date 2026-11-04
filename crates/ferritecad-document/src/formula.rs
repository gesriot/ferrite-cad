// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, Dimension, Quantity, Result, Unit, normalize_f64};
use std::ops::Range;

fn unit(name: &str) -> Option<Unit> {
    Some(match name {
        "mm" => Unit::Millimeter,
        "cm" => Unit::Centimeter,
        "m" => Unit::Meter,
        "in" => Unit::Inch,
        "ft" => Unit::Foot,
        "deg" => Unit::Degree,
        "rad" => Unit::Radian,
        _ => return None,
    })
}

fn identifier_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

pub(crate) fn check_name(name: &str) -> Result<()> {
    let mut bytes = name.bytes();
    if !bytes.next().is_some_and(identifier_start)
        || !bytes.all(|c| identifier_start(c) || c.is_ascii_digit())
    {
        return Err(CadError::input(format!("invalid parameter name '{name}'")));
    }
    if unit(name).is_some() {
        return Err(CadError::input(format!("'{name}' is reserved for a unit")));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Operator {
    Open,
    Add,
    Subtract,
    Multiply,
    Divide,
    Negate,
}

impl Operator {
    fn precedence(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::Add | Self::Subtract => 1,
            Self::Multiply | Self::Divide => 2,
            Self::Negate => 3,
        }
    }
}

enum Term {
    Number(f64, Unit),
    Name(Range<usize>),
    Apply(Operator),
}

pub(crate) struct Formula<'a> {
    text: &'a str,
    terms: Vec<Term>,
    literal: Option<(f64, Unit)>,
}

fn syntax(position: usize, reason: &str) -> CadError {
    CadError::input(format!("expression at byte {}: {reason}", position + 1))
}

fn digits(bytes: &[u8], i: &mut usize) {
    while bytes.get(*i).is_some_and(u8::is_ascii_digit) {
        *i += 1;
    }
}

fn skip_space(bytes: &[u8], i: &mut usize) {
    while bytes.get(*i).is_some_and(u8::is_ascii_whitespace) {
        *i += 1;
    }
}

fn identifier(bytes: &[u8], i: &mut usize) -> Range<usize> {
    let start = *i;
    while bytes
        .get(*i)
        .is_some_and(|&c| identifier_start(c) || c.is_ascii_digit())
    {
        *i += 1;
    }
    start..*i
}

fn number(text: &str, i: &mut usize) -> Result<Term> {
    let bytes = text.as_bytes();
    let start = *i;
    digits(bytes, i);
    if bytes.get(*i) == Some(&b'.') {
        *i += 1;
        digits(bytes, i);
    }
    if matches!(bytes.get(*i), Some(b'e' | b'E')) {
        *i += 1;
        if matches!(bytes.get(*i), Some(b'+' | b'-')) {
            *i += 1;
        }
        let exponent = *i;
        digits(bytes, i);
        if *i == exponent {
            return Err(syntax(*i, "expected exponent digits"));
        }
    }
    let value = text[start..*i]
        .parse()
        .map_err(|_| syntax(start, "invalid number"))?;
    skip_space(bytes, i);
    let suffix = if bytes.get(*i).is_some_and(|&c| identifier_start(c)) {
        let range = identifier(bytes, i);
        let name = &text[range.clone()];
        unit(name).ok_or_else(|| syntax(range.start, &format!("unknown unit '{name}'")))?
    } else {
        Unit::Unitless
    };
    Ok(Term::Number(value, suffix))
}

impl<'a> Formula<'a> {
    pub(crate) fn parse(text: &'a str) -> Result<Self> {
        let bytes = text.as_bytes();
        let mut i = 0;
        let mut terms = Vec::new();
        let mut ops = Vec::<(Operator, usize)>::new();
        let mut operand = true;
        let mut grouped = false;
        loop {
            skip_space(bytes, &mut i);
            let Some(&c) = bytes.get(i) else { break };
            if operand {
                match c {
                    b'(' | b'-' => {
                        let op = if c == b'(' {
                            grouped = true;
                            Operator::Open
                        } else {
                            Operator::Negate
                        };
                        ops.push((op, i));
                        i += 1;
                        continue;
                    }
                    b'0'..=b'9' | b'.' => terms.push(number(text, &mut i)?),
                    c if identifier_start(c) => {
                        let range = identifier(bytes, &mut i);
                        skip_space(bytes, &mut i);
                        if bytes.get(i) == Some(&b'(') {
                            return Err(syntax(i, "functions are not supported in expressions"));
                        }
                        terms.push(Term::Name(range));
                    }
                    _ => return Err(syntax(i, "expected a number, parameter or parenthesis")),
                }
                operand = false;
            } else if c == b')' {
                loop {
                    match ops.pop() {
                        Some((Operator::Open, _)) => break,
                        Some((op, _)) => terms.push(Term::Apply(op)),
                        None => return Err(syntax(i, "unmatched closing parenthesis")),
                    }
                }
                i += 1;
            } else {
                let op = match c {
                    b'+' => Operator::Add,
                    b'-' => Operator::Subtract,
                    b'*' => Operator::Multiply,
                    b'/' => Operator::Divide,
                    _ => return Err(syntax(i, "expected an operator")),
                };
                while ops
                    .last()
                    .is_some_and(|(last, _)| last.precedence() >= op.precedence())
                {
                    let (last, _) = ops.pop().expect("operator on stack");
                    terms.push(Term::Apply(last));
                }
                ops.push((op, i));
                i += 1;
                operand = true;
            }
        }
        if operand {
            return Err(syntax(i, "expected an operand"));
        }
        for (op, at) in ops.into_iter().rev() {
            if matches!(op, Operator::Open) {
                return Err(syntax(at, "unclosed parenthesis"));
            }
            terms.push(Term::Apply(op));
        }
        let literal = match terms.as_slice() {
            [Term::Number(n, u)] if !grouped => Some((*n, *u)),
            [Term::Number(n, u), Term::Apply(Operator::Negate)] if !grouped => Some((-n, *u)),
            _ => None,
        };
        Ok(Self {
            text,
            terms,
            literal,
        })
    }

    pub(crate) fn is_literal(&self) -> bool {
        self.literal.is_some()
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = (&str, Range<usize>)> {
        self.terms.iter().filter_map(|term| match term {
            Term::Name(r) => Some((&self.text[r.clone()], r.clone())),
            _ => None,
        })
    }

    pub(crate) fn renamed(&self, old: &str, new: &str) -> Option<String> {
        let mut ranges = self.names().filter(|(n, _)| *n == old).peekable();
        ranges.peek()?;
        let mut result = String::new();
        let mut start = 0;
        for (_, r) in ranges {
            result.push_str(&self.text[start..r.start]);
            result.push_str(new);
            start = r.end;
        }
        result.push_str(&self.text[start..]);
        Some(result)
    }

    pub(crate) fn value(
        &self,
        expected: Dimension,
        lookup: impl Fn(&str) -> Option<Quantity>,
    ) -> Result<f64> {
        if let Some((n, u)) = self.literal {
            let unit = if u == Unit::Unitless {
                expected.base_unit()
            } else {
                u
            };
            return check_dimension(Val::from(Quantity::new(n, unit)?), expected);
        }
        let mut stack: Vec<Val> = Vec::new();
        for term in &self.terms {
            let v =
                match term {
                    Term::Number(n, u) => Val::from(Quantity::new(*n, *u)?),
                    Term::Name(r) => {
                        let name = &self.text[r.clone()];
                        Val::from(lookup(name).ok_or_else(|| {
                            syntax(r.start, &format!("unknown parameter '{name}'"))
                        })?)
                    }
                    Term::Apply(op) => {
                        let rhs = stack.pop().expect("parsed operand");
                        if matches!(op, Operator::Negate) {
                            Val { n: -rhs.n, ..rhs }
                        } else {
                            stack.pop().expect("parsed left operand").binary(*op, rhs)?
                        }
                    }
                };
            stack.push(v);
        }
        check_dimension(stack.pop().expect("one parsed result"), expected)
    }
}

struct Val {
    n: f64,
    length: i64,
    angle: i64,
}

impl From<Quantity> for Val {
    fn from(q: Quantity) -> Self {
        Self {
            n: q.base_value(),
            length: i64::from(q.dimension() == Dimension::Length),
            angle: i64::from(q.dimension() == Dimension::Angle),
        }
    }
}

impl Val {
    fn binary(mut self, op: Operator, rhs: Self) -> Result<Self> {
        match op {
            Operator::Add | Operator::Subtract => {
                if self.length != rhs.length || self.angle != rhs.angle {
                    return Err(CadError::input(
                        "addition and subtraction require equal dimensions",
                    ));
                }
                self.n = if matches!(op, Operator::Add) {
                    self.n + rhs.n
                } else {
                    self.n - rhs.n
                };
            }
            Operator::Multiply => {
                self.n *= rhs.n;
                self.length += rhs.length;
                self.angle += rhs.angle;
            }
            Operator::Divide => {
                if rhs.n == 0. {
                    return Err(CadError::input("division by zero"));
                }
                self.n /= rhs.n;
                self.length -= rhs.length;
                self.angle -= rhs.angle;
            }
            Operator::Negate | Operator::Open => unreachable!("binary operator"),
        }
        self.n = normalize_f64(self.n)?;
        Ok(self)
    }
}

fn check_dimension(v: Val, expected: Dimension) -> Result<f64> {
    if v.length != i64::from(expected == Dimension::Length)
        || v.angle != i64::from(expected == Dimension::Angle)
    {
        return Err(CadError::input(format!(
            "expression must have dimension {expected}"
        )));
    }
    normalize_f64(v.n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_preserves_dimensions_order_and_unit_conversion_bits() -> Result<()> {
        let lookup = |name: &str| match name {
            "h" => Quantity::new(4., Unit::Millimeter).ok(),
            _ => None,
        };
        for (source, dimension, expected) in [
            (" -1.25e+1 ", Dimension::Length, -12.5),
            ("2 * h + h * h / (2 mm) - -1 cm", Dimension::Length, 26.),
            ("8 / 4 / 2", Dimension::Scalar, 1.),
            ("10 - 3 - 2", Dimension::Scalar, 5.),
            ("1 rad / h * 4mm", Dimension::Angle, 1.),
            ("-(h + 1 mm)", Dimension::Length, -5.),
        ] {
            assert_eq!(Formula::parse(source)?.value(dimension, lookup)?, expected);
        }
        for u in [
            Unit::Millimeter,
            Unit::Centimeter,
            Unit::Meter,
            Unit::Inch,
            Unit::Foot,
            Unit::Degree,
            Unit::Radian,
        ] {
            for gap in ["", " "] {
                let text = format!("30{gap}{}", u.symbol());
                let parsed = Formula::parse(&text)?;
                assert!(parsed.is_literal());
                assert_eq!(
                    parsed.value(u.dimension(), lookup)?.to_bits(),
                    Quantity::new(30., u)?.base_value().to_bits()
                );
            }
        }
        for text in ["(4)", "--4", "2+2", "h"] {
            assert!(!Formula::parse(text)?.is_literal());
        }
        Ok(())
    }

    #[test]
    fn syntax_and_graph_evaluation_fail_separately() {
        for (source, reason, position) in [
            ("4 + )", "expected a number", 5),
            ("1e+", "exponent digits", 4),
            ("(1 mm", "unclosed", 1),
            ("1)", "unmatched", 2),
            ("1 2", "expected an operator", 3),
            ("2 MM", "unknown unit", 3),
            ("sin (3)", "functions are not supported in expressions", 5),
            (".", "invalid number", 1),
            ("", "expected an operand", 1),
            ("2^3", "expected an operator", 2),
        ] {
            let e = Formula::parse(source)
                .err()
                .expect("invalid syntax")
                .to_string();
            assert!(
                e.contains(reason) && e.contains(&format!("byte {position}:")),
                "{e}"
            );
        }
        for (source, expected, reason) in [
            ("2 mm + 5", Dimension::Length, "equal dimensions"),
            ("3 cm / (1 - 1)", Dimension::Length, "division by zero"),
            ("2 * 25", Dimension::Length, "dimension length"),
            ("1 mm", Dimension::Angle, "dimension angle"),
            ("h", Dimension::Length, "unknown parameter 'h'"),
            ("1e308 * 1e10", Dimension::Scalar, "finite"),
        ] {
            let e = Formula::parse(source)
                .expect("syntax")
                .value(expected, |_| None)
                .expect_err("cannot evaluate")
                .to_string();
            assert!(e.contains(reason), "{source}: {e}");
        }
        for invalid in ["", "h.1", "2h", "mm", "deg", "höhe"] {
            assert!(check_name(invalid).is_err());
        }
        for valid in ["H", "h2", "_h", "MM"] {
            check_name(valid).expect("identifier");
        }
    }
}
