//! `no_std` lexer and recursive-descent parser for LES, built on `nom`.
//!
//! The grammar (lowest → highest precedence):
//!
//! ```text
//! or      := and  ( "or" | "||" ) and *
//! and     := comp ( "and" | "&&" ) comp *
//! comp    := add  ( "==" | "!=" | "<=" | ">=" | "<" | ">" ) add *
//! add     := mul  ( "+" | "-" ) mul *
//! mul     := un   ( "*" | "/" | "%" ) un *
//! un      := ( "-" | "not" | "!" ) un | pow
//! pow     := post ( "^" un )?              // right-associative
//! post    := atom ( "(" args ")" )*        // function / aggregate calls
//! atom    := number | string | bool | cellref | name | "(" expr ")"
//! ```

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{alphanumeric1, anychar, char, digit1, multispace0, one_of};
use nom::combinator::{map, not, opt, recognize, value};
use nom::error::{Error as NomError, ErrorKind};
use nom::multi::many1;
use nom::sequence::{delimited, preceded, terminated, tuple};
use nom::IResult;

use tpt_lattice_core::{CellId, LatticeError};

use crate::ast::{
    BinaryOp, CastKind, CellRef, Expr, Formula, Literal, MatchArm, MatchPattern, UnaryOp,
};

type P<'a, O> = IResult<&'a str, O>;

/// Strip optional leading `=` and parse a formula string.
pub fn parse(input: &str) -> Result<Formula, LatticeError> {
    let trimmed = input.trim();
    let body = trimmed.strip_prefix('=').unwrap_or(trimmed);
    match all_consumed(body) {
        Ok(formula) => Ok(formula),
        Err(_) => Err(LatticeError::ParseError(input.to_string())),
    }
}

/// Parse a bare expression (no leading `=` required).
pub fn parse_expr(input: &str) -> Result<Expr, LatticeError> {
    match all_consumed_expr(input.trim()) {
        Ok(expr) => Ok(expr),
        Err(_) => Err(LatticeError::ParseError(input.to_string())),
    }
}

fn all_consumed(input: &str) -> Result<Formula, ()> {
    match nom::combinator::all_consuming(ws(p_expr))(input) {
        Ok((_, expr)) => Ok(Formula { body: expr }),
        Err(_) => Err(()),
    }
}

fn all_consumed_expr(input: &str) -> Result<Expr, ()> {
    match nom::combinator::all_consuming(ws(p_expr))(input) {
        Ok((_, expr)) => Ok(expr),
        Err(_) => Err(()),
    }
}

/// Wrap a parser to ignore surrounding whitespace.
fn ws<'a, F, O>(mut f: F) -> impl FnMut(&'a str) -> P<'a, O>
where
    F: FnMut(&'a str) -> P<'a, O>,
{
    move |input: &'a str| {
        let (i, _) = multispace0(input)?;
        let (i, o) = f(i)?;
        let (i, _) = multispace0(i)?;
        Ok((i, o))
    }
}

fn fail<'a, O>(input: &'a str, _msg: &'static str) -> P<'a, O> {
    Err(nom::Err::Error(NomError::new(input, ErrorKind::Fail)))
}

// ---------------------------------------------------------------------------
// Atoms
// ---------------------------------------------------------------------------

fn p_number(input: &str) -> P<'_, Expr> {
    let (rest, s) = recognize(tuple((
        digit1,
        opt(preceded(char('.'), digit1)),
        opt(tuple((one_of("eE"), opt(one_of("+-")), digit1))),
    )))(input)?;
    match s.parse::<f64>() {
        Ok(n) => Ok((rest, Expr::Literal(Literal::Number(n)))),
        Err(_) => fail(input, "invalid number literal"),
    }
}

fn p_string(input: &str) -> P<'_, Expr> {
    let (mut i, _) = char('"')(input)?;
    let mut out = String::new();
    loop {
        if let Ok((rest, _)) = char::<_, NomError<&str>>('"')(i) {
            return Ok((rest, Expr::Literal(Literal::Text(out))));
        }
        if let Ok((rest, c)) =
            preceded(char('\\'), one_of::<&str, &str, NomError<&str>>("\"\\nrt"))(i)
        {
            let c = match c {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            };
            out.push(c);
            i = rest;
            continue;
        }
        if let Ok((rest, c)) = anychar::<&str, NomError<&str>>(i) {
            out.push(c);
            i = rest;
            continue;
        }
        return fail(i, "unterminated string");
    }
}

fn ident(input: &str) -> P<'_, String> {
    if let Ok((rest, s)) = recognize(tuple((alpha, alphanumeric1)))(input) {
        return Ok((rest, s.to_string()));
    }
    // single-letter identifier (no trailing alphanumeric)
    let (rest, c) = alpha(input)?;
    let mut s = String::new();
    s.push(c);
    Ok((rest, s))
}

fn alpha(input: &str) -> P<'_, char> {
    one_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ")(input)
}

fn p_atom_ident(input: &str) -> P<'_, Expr> {
    // Try an (optionally absolute, `$A$1`) cell reference first. On failure it
    // backtracks to the original input, so the name/false/true branches below run.
    if let Ok((rest, expr)) = p_cellref(input) {
        return Ok((rest, expr));
    }
    let (input, name) = ident(input)?;
    if name.eq_ignore_ascii_case("true") {
        return Ok((input, Expr::Literal(Literal::Boolean(true))));
    }
    if name.eq_ignore_ascii_case("false") {
        return Ok((input, Expr::Literal(Literal::Boolean(false))));
    }
    match CellId::try_from_a1(&name) {
        Ok(id) => Ok((
            input,
            Expr::CellRef(CellRef {
                id,
                a1: name.clone(),
                abs_col: false,
                abs_row: false,
                sheet: None,
            }),
        )),
        Err(_) => Ok((input, Expr::Name(name))),
    }
}

/// Parse an A1 cell reference with optional `$` absolute markers: `$A$1`, `A$1`,
/// `$A1`, or `A1`. Returns [`Expr::CellRef`] with the absolute flags recorded.
fn p_cellref(input: &str) -> P<'_, Expr> {
    let (rest, abs_col) = opt(tag("$"))(input)?;
    let abs_col = abs_col.is_some();
    let (rest, col) = recognize(many1(one_of(
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
    )))(rest)?;
    let (rest, abs_row) = opt(tag("$"))(rest)?;
    let abs_row = abs_row.is_some();
    let (rest, row) = digit1(rest)?;
    let a1 = format!(
        "{}{}{}{}",
        if abs_col { "$" } else { "" },
        col,
        if abs_row { "$" } else { "" },
        row
    );
    match CellId::try_from_a1(&a1) {
        Ok(id) => Ok((
            rest,
            Expr::CellRef(CellRef {
                id,
                a1,
                abs_col,
                abs_row,
                sheet: None,
            }),
        )),
        Err(_) => fail(rest, "invalid cell reference"),
    }
}

fn p_paren(input: &str) -> P<'_, Expr> {
    delimited(ws(char('(')), ws(p_expr), ws(char(')')))(input)
}

fn p_atom(input: &str) -> P<'_, Expr> {
    // Optional leading `Sheet!` qualifier for cross-sheet references.
    let (input, sheet) = opt(p_sheet)(input)?;
    let (input, base) = alt((p_number, p_string, p_paren, p_atom_ident))(input)?;
    let base = match base {
        Expr::CellRef(mut c) => {
            c.sheet = sheet;
            Expr::CellRef(c)
        }
        Expr::Range { mut start, mut end } => {
            start.sheet = sheet.clone();
            end.sheet = sheet;
            Expr::Range { start, end }
        }
        other => other,
    };
    Ok((input, base))
}

/// Parse an optional `Sheet!` prefix (an identifier immediately followed by `!`,
/// but not `!=`). Returns the sheet name.
fn p_sheet(input: &str) -> P<'_, String> {
    let (rest, name) = ident(input)?;
    let (rest, _) = char('!')(rest)?;
    // `!=` is a binary operator, not a sheet prefix.
    if rest.starts_with('=') {
        return fail(rest, "not a sheet prefix");
    }
    Ok((rest, name))
}

// ---------------------------------------------------------------------------
// Postfix (function / aggregate calls)
// ---------------------------------------------------------------------------

fn p_arm(input: &str) -> P<'_, MatchArm> {
    alt((
        map(
            tuple((
                ws(tag("Ok")),
                char('('),
                ident,
                char(')'),
                ws(tag("=>")),
                ws(p_expr),
            )),
            |(_, _, name, _, _, body)| MatchArm {
                pattern: MatchPattern::Ok(name),
                body,
            },
        ),
        map(
            tuple((
                ws(tag("Err")),
                char('('),
                ident,
                char(')'),
                ws(tag("=>")),
                ws(p_expr),
            )),
            |(_, _, name, _, _, body)| MatchArm {
                pattern: MatchPattern::Err(name),
                body,
            },
        ),
        map(
            tuple((ws(tag("_")), ws(tag("=>")), ws(p_expr))),
            |(_, _, body)| MatchArm {
                pattern: MatchPattern::Wildcard,
                body,
            },
        ),
    ))(input)
}

fn p_match_call(input: &str) -> P<'_, Expr> {
    let (mut i, scrutinee) = ws(p_expr)(input)?;
    let mut arms: Vec<MatchArm> = Vec::new();
    if let Ok((i2, _)) = ws(char(','))(i) {
        i = i2;
        let (i3, a) = nom::multi::separated_list1(ws(char(',')), ws(p_arm))(i)?;
        arms.extend(a);
        i = i3;
    }
    let (i, _) = char(')')(i)?;
    Ok((
        i,
        Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
        },
    ))
}

fn p_postfix(input: &str) -> P<'_, Expr> {
    let (mut input, mut base) = p_atom(input)?;
    loop {
        let (rest, _) = match ws(char('('))(input) {
            Ok(v) => v,
            Err(_) => break,
        };
        if let Expr::Name(n) = &base {
            if n.eq_ignore_ascii_case("MATCH") {
                let (r, e) = p_match_call(rest)?;
                base = e;
                input = r;
                continue;
            }
        }
        let (rest2, args) = terminated(
            nom::multi::separated_list0(ws(char(',')), ws(p_expr)),
            ws(char(')')),
        )(rest)?;
        match base {
            Expr::Name(n) => {
                let up = n.to_uppercase();
                match up.as_str() {
                    "RANGE" => {
                        if args.len() != 2 {
                            return fail(input, "RANGE expects two cell refs");
                        }
                        match (&args[0], &args[1]) {
                            (Expr::CellRef(a), Expr::CellRef(b)) => {
                                base = Expr::Range {
                                    start: a.clone(),
                                    end: b.clone(),
                                };
                            }
                            _ => return fail(input, "RANGE expects cell refs"),
                        }
                    }
                    "NUMBER" | "TEXT" | "BOOLEAN" => {
                        if args.len() != 1 {
                            return fail(input, "cast expects one argument");
                        }
                        let kind = match up.as_str() {
                            "NUMBER" => CastKind::Number,
                            "TEXT" => CastKind::Text,
                            _ => CastKind::Boolean,
                        };
                        base = Expr::Cast {
                            kind,
                            expr: Box::new(args.into_iter().next().unwrap()),
                        };
                    }
                    _ => base = Expr::Call { name: n, args },
                }
            }
            _ => return fail(input, "cannot invoke a non-name"),
        }
        input = rest2;
    }
    Ok((input, base))
}

// ---------------------------------------------------------------------------
// Unary / power
// ---------------------------------------------------------------------------

fn p_power(input: &str) -> P<'_, Expr> {
    let (mut input, mut base) = p_postfix(input)?;
    if let Ok((rest, _)) = ws(char('^'))(input) {
        let (rest, rhs) = p_unary(rest)?;
        base = Expr::Binary {
            op: BinaryOp::Pow,
            left: Box::new(base),
            right: Box::new(rhs),
        };
        input = rest;
    }
    Ok((input, base))
}

fn p_unary(input: &str) -> P<'_, Expr> {
    alt((
        map(preceded(ws(char('-')), p_unary), |e| Expr::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(e),
        }),
        map(
            preceded(ws(value((), alt((tag("not"), tag("!"))))), p_unary),
            |e| Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(e),
            },
        ),
        p_power,
    ))(input)
}

// ---------------------------------------------------------------------------
// Binary precedence ladder
// ---------------------------------------------------------------------------

struct OpSpec {
    sym: &'static str,
    op: BinaryOp,
    keyword: bool,
}

fn try_op<'a>(input: &'a str, spec: &OpSpec) -> Option<&'a str> {
    if spec.keyword {
        let (rest, _) = ws(terminated(tag(spec.sym), not(alphanumeric1)))(input).ok()?;
        Some(rest)
    } else {
        let (rest, _) = ws(tag(spec.sym))(input).ok()?;
        Some(rest)
    }
}

fn fold_op<'a>(
    mut input: &'a str,
    mut lhs: Expr,
    sub: fn(&'a str) -> P<'a, Expr>,
    specs: &[OpSpec],
) -> P<'a, Expr> {
    loop {
        let mut chosen: Option<(BinaryOp, &'a str)> = None;
        for spec in specs {
            if let Some(rest) = try_op(input, spec) {
                chosen = Some((spec.op, rest));
                break;
            }
        }
        match chosen {
            Some((op, rest)) => {
                let (rest, rhs) = sub(rest)?;
                lhs = Expr::Binary {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                };
                input = rest;
            }
            None => return Ok((input, lhs)),
        }
    }
}

fn p_mul(input: &str) -> P<'_, Expr> {
    let (input, lhs) = p_unary(input)?;
    fold_op(
        input,
        lhs,
        p_unary,
        &[
            OpSpec {
                sym: "*",
                op: BinaryOp::Mul,
                keyword: false,
            },
            OpSpec {
                sym: "/",
                op: BinaryOp::Div,
                keyword: false,
            },
            OpSpec {
                sym: "%",
                op: BinaryOp::Mod,
                keyword: false,
            },
        ],
    )
}

fn p_add(input: &str) -> P<'_, Expr> {
    let (input, lhs) = p_mul(input)?;
    fold_op(
        input,
        lhs,
        p_mul,
        &[
            OpSpec {
                sym: "+",
                op: BinaryOp::Add,
                keyword: false,
            },
            OpSpec {
                sym: "-",
                op: BinaryOp::Sub,
                keyword: false,
            },
        ],
    )
}

fn p_concat(input: &str) -> P<'_, Expr> {
    let (input, lhs) = p_add(input)?;
    fold_op(
        input,
        lhs,
        p_add,
        &[OpSpec {
            sym: "&",
            op: BinaryOp::Concat,
            keyword: false,
        }],
    )
}

fn p_comp(input: &str) -> P<'_, Expr> {
    let (input, lhs) = p_concat(input)?;
    fold_op(
        input,
        lhs,
        p_concat,
        &[
            OpSpec {
                sym: "==",
                op: BinaryOp::Eq,
                keyword: false,
            },
            OpSpec {
                sym: "!=",
                op: BinaryOp::Ne,
                keyword: false,
            },
            OpSpec {
                sym: "<=",
                op: BinaryOp::Le,
                keyword: false,
            },
            OpSpec {
                sym: ">=",
                op: BinaryOp::Ge,
                keyword: false,
            },
            OpSpec {
                sym: "<",
                op: BinaryOp::Lt,
                keyword: false,
            },
            OpSpec {
                sym: ">",
                op: BinaryOp::Gt,
                keyword: false,
            },
        ],
    )
}

fn p_and(input: &str) -> P<'_, Expr> {
    let (input, lhs) = p_comp(input)?;
    fold_op(
        input,
        lhs,
        p_comp,
        &[
            OpSpec {
                sym: "and",
                op: BinaryOp::And,
                keyword: true,
            },
            OpSpec {
                sym: "&&",
                op: BinaryOp::And,
                keyword: false,
            },
        ],
    )
}

fn p_or(input: &str) -> P<'_, Expr> {
    let (input, lhs) = p_and(input)?;
    fold_op(
        input,
        lhs,
        p_and,
        &[
            OpSpec {
                sym: "or",
                op: BinaryOp::Or,
                keyword: true,
            },
            OpSpec {
                sym: "||",
                op: BinaryOp::Or,
                keyword: false,
            },
        ],
    )
}

fn p_expr(input: &str) -> P<'_, Expr> {
    p_or(input)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Expr {
        parse_expr(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"))
    }

    #[test]
    fn literals() {
        assert_eq!(ok("42"), Expr::Literal(Literal::Number(42.0)));
        assert_eq!(ok("2.5"), Expr::Literal(Literal::Number(2.5)));
        assert_eq!(
            ok("-5"),
            Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(Expr::Literal(Literal::Number(5.0)))
            }
        );
        assert_eq!(ok("\"hi\""), Expr::Literal(Literal::Text("hi".into())));
        assert_eq!(ok("true"), Expr::Literal(Literal::Boolean(true)));
    }

    #[test]
    fn cell_refs() {
        match ok("A1") {
            Expr::CellRef(c) => assert_eq!(c.a1, "A1"),
            other => panic!("expected cell ref, got {other:?}"),
        }
    }

    #[test]
    fn precedence() {
        // 1 + 2 * 3 == 1 + (2*3)
        match ok("1 + 2 * 3") {
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => {
                assert_eq!(*left, Expr::Literal(Literal::Number(1.0)));
                assert!(matches!(
                    right.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::Mul,
                        ..
                    }
                ));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn power_right_assoc() {
        match ok("2 ^ 3 ^ 2") {
            Expr::Binary {
                op: BinaryOp::Pow,
                left,
                right,
            } => {
                assert_eq!(*left, Expr::Literal(Literal::Number(2.0)));
                assert!(matches!(
                    right.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::Pow,
                        ..
                    }
                ));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn range_and_calls() {
        match ok("RANGE(A1, B2)") {
            Expr::Range { start, end } => {
                assert_eq!(start.a1, "A1");
                assert_eq!(end.a1, "B2");
            }
            other => panic!("got {other:?}"),
        }
        match ok("SUM(A1, A2)") {
            Expr::Call { name, args } => {
                assert_eq!(name, "SUM");
                assert_eq!(args.len(), 2);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn match_expr() {
        match ok("MATCH(A1, Ok(v) => v * 2, Err(e) => 0)") {
            Expr::Match { scrutinee, arms } => {
                assert!(matches!(scrutinee.as_ref(), Expr::CellRef(_)));
                assert_eq!(arms.len(), 2);
                assert!(matches!(arms[0].pattern, MatchPattern::Ok(_)));
                assert!(matches!(arms[1].pattern, MatchPattern::Err(_)));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn casts() {
        match ok("NUMBER(\"5\") + 5") {
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                ..
            } => {
                assert!(matches!(
                    left.as_ref(),
                    Expr::Cast {
                        kind: CastKind::Number,
                        ..
                    }
                ));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn keyword_boundary() {
        // "orbit" must not be parsed as "or" + "bit"
        match ok("orbit") {
            Expr::Name(n) => assert_eq!(n, "orbit"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn errors() {
        assert!(parse("1 +").is_err());
        assert!(parse("RANGE(A1)").is_err());
        assert!(parse("SUM(").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn roundtrip_display() {
        for src in [
            "1 + 2 * 3",
            "RANGE(A1, B2)",
            "MATCH(A1, Ok(v) => v * 2, Err(e) => 0)",
            "NUMBER(\"5\") + 5",
            "-2 ^ 3",
            "A1 and not B2",
            "A1 & \"x\"",
            "A1 & B1 & C1",
        ] {
            let e = ok(src);
            let printed = e.to_string();
            let reparsed = parse_expr(&printed).expect("re-parse");
            assert_eq!(e, reparsed, "roundtrip failed for {src} -> {printed}");
        }
    }
}
