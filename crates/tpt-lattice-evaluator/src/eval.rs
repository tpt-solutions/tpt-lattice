//! Evaluation of a parsed [`Expr`] against a [`GridState`].

use std::collections::HashMap;

use tpt_lattice_core::{CellId, CellValue, GridState, LatticeError};
use tpt_lattice_parser::ast::{
    BinaryOp, CastKind, CellRef, Expr, Literal, MatchPattern, UnaryOp,
};

use crate::dag::MAX_RANGE_CELLS;

/// Evaluate a formula expression against `grid`, resolving `MATCH` bindings via
/// `env`. Reads of other cells go through `grid` (so the evaluator is agnostic
/// to whether storage is in-memory, CRDT-backed, or deserialized).
pub fn eval_expr(
    expr: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    match expr {
        Expr::Literal(l) => match l {
            Literal::Number(n) => CellValue::Number(*n),
            Literal::Text(s) => CellValue::Text(s.clone()),
            Literal::Boolean(b) => CellValue::Boolean(*b),
            Literal::Error(e) => CellValue::Error(e.clone()),
        },
        Expr::CellRef(c) => grid.get_cell(c.id).sanitize(),
        Expr::Name(n) => match env.get(n) {
            Some(v) => v.clone(),
            None => CellValue::Error(LatticeError::name_error(n.clone())),
        },
        Expr::Unary { op, expr } => eval_unary(*op, expr, grid, env),
        Expr::Binary { op, left, right } => eval_binary(*op, left, right, grid, env),
        Expr::Call { name, args } => call_function(name, args, grid, env),
        Expr::Range { .. } => CellValue::Error(LatticeError::internal(
            "RANGE used outside of a function argument",
        )),
        Expr::Cast { kind, expr } => eval_cast(*kind, expr, grid, env),
        Expr::Match { scrutinee, arms } => eval_match(scrutinee, arms, grid, env),
    }
}

fn eval_unary(
    op: UnaryOp,
    expr: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    let v = eval_expr(expr, grid, env);
    match op {
        UnaryOp::Neg => match v {
            CellValue::Number(n) => CellValue::Number(-n),
            CellValue::Error(e) => CellValue::Error(e),
            other => CellValue::Error(LatticeError::type_error("Number", variant_name(&other))),
        },
        UnaryOp::Not => match v {
            CellValue::Boolean(b) => CellValue::Boolean(!b),
            CellValue::Error(e) => CellValue::Error(e),
            other => CellValue::Error(LatticeError::type_error("Boolean", variant_name(&other))),
        },
    }
}

fn eval_binary(
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    // Short-circuit boolean operators on errors / booleans.
    if matches!(
        op,
        BinaryOp::And | BinaryOp::Or | BinaryOp::Eq | BinaryOp::Ne
    ) {
        let l = eval_expr(left, grid, env);
        if let CellValue::Error(e) = l {
            return CellValue::Error(e);
        }
        let r = eval_expr(right, grid, env);
        if let CellValue::Error(e) = r {
            return CellValue::Error(e);
        }
        return eval_binary_values(op, l, r);
    }

    let l = eval_expr(left, grid, env);
    let r = eval_expr(right, grid, env);
    eval_binary_values(op, l, r)
}

fn eval_binary_values(op: BinaryOp, l: CellValue, r: CellValue) -> CellValue {
    use CellValue::*;
    match op {
        BinaryOp::Add => arith(l, r, |a, b| a + b),
        BinaryOp::Sub => arith(l, r, |a, b| a - b),
        BinaryOp::Mul => arith(l, r, |a, b| a * b),
        BinaryOp::Div => {
            if let (Number(a), Number(b)) = (&l, &r) {
                if *b == 0.0 {
                    return Error(LatticeError::DivByZero);
                }
                return Number(a / b);
            }
            mismatch(op, l, r)
        }
        BinaryOp::Mod => {
            if let (Number(a), Number(b)) = (&l, &r) {
                if *b == 0.0 {
                    return Error(LatticeError::DivByZero);
                }
                return Number(a % b);
            }
            mismatch(op, l, r)
        }
        BinaryOp::Pow => arith(l, r, |a, b| a.powf(b)),
        BinaryOp::Eq => bool_of(l == r),
        BinaryOp::Ne => bool_of(l != r),
        BinaryOp::Lt => cmp(l, r, |o: std::cmp::Ordering| o == std::cmp::Ordering::Less),
        BinaryOp::Le => cmp(l, r, |o| o != std::cmp::Ordering::Greater),
        BinaryOp::Gt => cmp(l, r, |o| o == std::cmp::Ordering::Greater),
        BinaryOp::Ge => cmp(l, r, |o| o != std::cmp::Ordering::Less),
        BinaryOp::And => logical(l, r, |a, b| a && b),
        BinaryOp::Or => logical(l, r, |a, b| a || b),
    }
}

fn arith<F: Fn(f64, f64) -> f64>(l: CellValue, r: CellValue, f: F) -> CellValue {
    match (l, r) {
        (CellValue::Number(a), CellValue::Number(b)) => CellValue::Number(f(a, b)),
        (CellValue::Error(e), _) | (_, CellValue::Error(e)) => CellValue::Error(e),
        (a, b) => mismatch(BinaryOp::Add, a, b),
    }
}

fn logical<F: Fn(bool, bool) -> bool>(l: CellValue, r: CellValue, f: F) -> CellValue {
    match (l, r) {
        (CellValue::Boolean(a), CellValue::Boolean(b)) => CellValue::Boolean(f(a, b)),
        (CellValue::Error(e), _) | (_, CellValue::Error(e)) => CellValue::Error(e),
        (a, b) => mismatch(BinaryOp::And, a, b),
    }
}

fn cmp<F: Fn(std::cmp::Ordering) -> bool>(l: CellValue, r: CellValue, pred: F) -> CellValue {
    use std::cmp::Ordering;
    match (&l, &r) {
        (CellValue::Number(a), CellValue::Number(b)) => bool_of(pred(a.partial_cmp(b).unwrap_or(Ordering::Equal))),
        (CellValue::Text(a), CellValue::Text(b)) => bool_of(pred(a.cmp(b))),
        (CellValue::Boolean(a), CellValue::Boolean(b)) => bool_of(pred(a.cmp(b))),
        (CellValue::Error(e), _) | (_, CellValue::Error(e)) => CellValue::Error(e.clone()),
        _ => CellValue::Error(LatticeError::type_error(
            "comparable values",
            format!("{} and {}", variant_name(&l), variant_name(&r)),
        )),
    }
}

fn mismatch(op: BinaryOp, l: CellValue, r: CellValue) -> CellValue {
    CellValue::Error(LatticeError::type_error(
        format!("operands for {}", op.symbol()),
        format!("{} and {}", variant_name(&l), variant_name(&r)),
    ))
}

fn bool_of(b: bool) -> CellValue {
    CellValue::Boolean(b)
}

fn variant_name(v: &CellValue) -> &'static str {
    match v {
        CellValue::Empty => "Empty",
        CellValue::Number(_) => "Number",
        CellValue::Text(_) => "Text",
        CellValue::Boolean(_) => "Boolean",
        CellValue::Error(_) => "Error",
    }
}

// ---------------------------------------------------------------------------
// Casts & MATCH
// ---------------------------------------------------------------------------

fn eval_cast(
    kind: CastKind,
    expr: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    let v = eval_expr(expr, grid, env);
    match kind {
        CastKind::Number => match v {
            CellValue::Number(n) => CellValue::Number(n),
            CellValue::Text(s) => match s.parse::<f64>() {
                Ok(n) if n.is_finite() => CellValue::Number(n),
                _ => CellValue::Error(LatticeError::NotANumber),
            },
            CellValue::Boolean(b) => CellValue::Number(if b { 1.0 } else { 0.0 }),
            CellValue::Error(e) => CellValue::Error(e),
            CellValue::Empty => CellValue::Error(LatticeError::type_error("Number", "Empty")),
        },
        CastKind::Text => match v {
            CellValue::Text(s) => CellValue::Text(s),
            CellValue::Number(n) => CellValue::Text(n.to_string()),
            CellValue::Boolean(b) => CellValue::Text(b.to_string()),
            CellValue::Error(e) => CellValue::Text(format!("#{e}")),
            CellValue::Empty => CellValue::Text(String::new()),
        },
        CastKind::Boolean => match v {
            CellValue::Boolean(b) => CellValue::Boolean(b),
            CellValue::Number(n) => CellValue::Boolean(n != 0.0),
            CellValue::Text(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" => CellValue::Boolean(true),
                "false" => CellValue::Boolean(false),
                "" => CellValue::Boolean(false),
                _ => CellValue::Error(LatticeError::type_error("Boolean", "Text")),
            },
            CellValue::Error(e) => CellValue::Error(e),
            CellValue::Empty => CellValue::Error(LatticeError::type_error("Boolean", "Empty")),
        },
    }
}

fn eval_match(
    scrutinee: &Expr,
    arms: &[tpt_lattice_parser::ast::MatchArm],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    let value = eval_expr(scrutinee, grid, env);
    let is_err = value.is_error();
    for arm in arms {
        match (&arm.pattern, &value) {
            (MatchPattern::Ok(name), v) if !v.is_error() => {
                return with_binding(name, v.clone(), &arm.body, grid, env);
            }
            (MatchPattern::Err(name), CellValue::Error(_)) => {
                return with_binding(name, value.clone(), &arm.body, grid, env);
            }
            (MatchPattern::Wildcard, _) => {
                return eval_expr(&arm.body, grid, env);
            }
            _ => continue,
        }
    }
    CellValue::Error(LatticeError::internal(if is_err {
        "no Err arm matched the error"
    } else {
        "no Ok/wildcard arm matched the value"
    }))
}

fn with_binding(
    name: &str,
    value: CellValue,
    body: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    let prev = env.insert(name.to_string(), value);
    let result = eval_expr(body, grid, env);
    match prev {
        Some(p) => {
            env.insert(name.to_string(), p);
        }
        None => {
            env.remove(name);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Built-in functions
// ---------------------------------------------------------------------------

fn call_function(
    name: &str,
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    match name.to_ascii_uppercase().as_str() {
        "SUM" => reduce_numbers(args, grid, env, 0.0, |a, b| a + b, "SUM"),
        "PRODUCT" => reduce_numbers(args, grid, env, 1.0, |a, b| a * b, "PRODUCT"),
        "MIN" => min_max(args, grid, env, true),
        "MAX" => min_max(args, grid, env, false),
        "AVERAGE" => average(args, grid, env),
        "COUNT" => count(args, grid, env),
        "ABS" => unary_num(args, grid, env, |a| a.abs(), "ABS"),
        "SQRT" => unary_num(args, grid, env, |a| a.sqrt(), "SQRT"),
        "FLOOR" => unary_num(args, grid, env, |a| a.floor(), "FLOOR"),
        "CEIL" => unary_num(args, grid, env, |a| a.ceil(), "CEIL"),
        "ROUND" => round(args, grid, env),
        "MOD" => binary_num(args, grid, env, |a, b| a % b, "MOD"),
        "POW" => binary_num(args, grid, env, |a, b| a.powf(b), "POW"),
        "CONCAT" => concat(args, grid, env),
        "LEN" => len(args, grid, env),
        "IF" => if_fn(args, grid, env),
        "AND" => fold_bool(args, grid, env, true, |a, b| a && b),
        "OR" => fold_bool(args, grid, env, false, |a, b| a || b),
        "NOT" => unary_bool(args, grid, env),
        "NUMBER" => eval_cast(CastKind::Number, &args[0], grid, env),
        "TEXT" => eval_cast(CastKind::Text, &args[0], grid, env),
        "BOOLEAN" => eval_cast(CastKind::Boolean, &args[0], grid, env),
        other => CellValue::Error(LatticeError::name_error(other)),
    }
}

/// Expand a `RANGE(start, end)` into the cell ids it covers (row-major).
fn expand_range(start: &CellRef, end: &CellRef, out: &mut Vec<CellId>) -> Result<(), LatticeError> {
    let (sc, sr) = start.id.to_rc();
    let (ec, er) = end.id.to_rc();
    let (c0, c1) = (sc.min(ec), sc.max(ec));
    let (r0, r1) = (sr.min(er), sr.max(er));
    let count = (c1 - c0 + 1) as usize * (r1 - r0 + 1) as usize;
    if count > MAX_RANGE_CELLS {
        return Err(LatticeError::argument_error("RANGE spans too many cells"));
    }
    for r in r0..=r1 {
        for c in c0..=c1 {
            out.push(CellId::from_rc(c, r));
        }
    }
    Ok(())
}

/// Collect numeric values referenced by the argument expressions.
fn collect_numbers(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> Result<Vec<f64>, LatticeError> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Expr::Range { start, end } => {
                let mut ids = Vec::new();
                expand_range(start, end, &mut ids)?;
                for id in ids {
                    if let CellValue::Number(n) = grid.get_cell(id) {
                        out.push(n);
                    }
                }
            }
            Expr::CellRef(c) => {
                if let CellValue::Number(n) = grid.get_cell(c.id) {
                    out.push(n);
                }
            }
            other => match eval_expr(other, grid, env) {
                CellValue::Number(n) => out.push(n),
                CellValue::Error(e) => return Err(e),
                _ => {}
            },
        }
    }
    Ok(out)
}

fn reduce_numbers(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    init: f64,
    f: impl Fn(f64, f64) -> f64,
    name: &str,
) -> CellValue {
    if args.is_empty() {
        return CellValue::Error(LatticeError::argument_error(format!("{name} expects at least one argument")));
    }
    match collect_numbers(args, grid, env) {
        Ok(nums) => CellValue::Number(nums.into_iter().fold(init, &f)),
        Err(e) => CellValue::Error(e),
    }
}

fn min_max(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>, is_min: bool) -> CellValue {
    match collect_numbers(args, grid, env) {
        Ok(nums) => {
            if nums.is_empty() {
                return CellValue::Empty;
            }
            let r = if is_min {
                nums.iter().copied().fold(f64::INFINITY, f64::min)
            } else {
                nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            };
            CellValue::Number(r)
        }
        Err(e) => CellValue::Error(e),
    }
}

fn average(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    match collect_numbers(args, grid, env) {
        Ok(nums) => {
            if nums.is_empty() {
                return CellValue::Empty;
            }
            let sum: f64 = nums.iter().sum();
            CellValue::Number(sum / nums.len() as f64)
        }
        Err(e) => CellValue::Error(e),
    }
}

fn count(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    match collect_numbers(args, grid, env) {
        Ok(nums) => CellValue::Number(nums.len() as f64),
        Err(e) => CellValue::Error(e),
    }
}

fn unary_num(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    f: impl Fn(f64) -> f64,
    name: &str,
) -> CellValue {
    if args.len() != 1 {
        return CellValue::Error(LatticeError::argument_error(format!("{name} expects one argument")));
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Number(n) => CellValue::Number(f(n)),
        CellValue::Error(e) => CellValue::Error(e),
        other => CellValue::Error(LatticeError::type_error("Number", variant_name(&other))),
    }
}

fn binary_num(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    f: impl Fn(f64, f64) -> f64,
    name: &str,
) -> CellValue {
    if args.len() != 2 {
        return CellValue::Error(LatticeError::argument_error(format!("{name} expects two arguments")));
    }
    let a = eval_expr(&args[0], grid, env);
    let b = eval_expr(&args[1], grid, env);
    match (a, b) {
        (CellValue::Number(a), CellValue::Number(b)) => CellValue::Number(f(a, b)),
        (CellValue::Error(e), _) | (_, CellValue::Error(e)) => CellValue::Error(e),
        (a, b) => mismatch(BinaryOp::Add, a, b),
    }
}

fn round(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 1 && args.len() != 2 {
        return CellValue::Error(LatticeError::argument_error("ROUND expects 1 or 2 arguments"));
    }
    let n = match eval_expr(&args[0], grid, env) {
        CellValue::Number(n) => n,
        CellValue::Error(e) => return CellValue::Error(e),
        other => return CellValue::Error(LatticeError::type_error("Number", variant_name(&other))),
    };
    let digits = if args.len() == 2 {
        match eval_expr(&args[1], grid, env) {
            CellValue::Number(d) => d as i32,
            CellValue::Error(e) => return CellValue::Error(e),
            other => return CellValue::Error(LatticeError::type_error("Number", variant_name(&other))),
        }
    } else {
        0
    };
    let factor = 10f64.powi(digits);
    CellValue::Number((n * factor).round() / factor)
}

fn concat(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    let mut out = String::new();
    for arg in args {
        match eval_expr(arg, grid, env) {
            CellValue::Text(s) => out.push_str(&s),
            CellValue::Number(n) => out.push_str(&n.to_string()),
            CellValue::Boolean(b) => out.push_str(&b.to_string()),
            CellValue::Error(e) => return CellValue::Error(e),
            CellValue::Empty => {}
        }
    }
    CellValue::Text(out)
}

fn len(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 1 {
        return CellValue::Error(LatticeError::argument_error("LEN expects one argument"));
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Text(s) => CellValue::Number(s.chars().count() as f64),
        CellValue::Error(e) => CellValue::Error(e),
        other => CellValue::Error(LatticeError::type_error("Text", variant_name(&other))),
    }
}

fn if_fn(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 3 {
        return CellValue::Error(LatticeError::argument_error("IF expects (cond, then, else)"));
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Boolean(true) => eval_expr(&args[1], grid, env),
        CellValue::Boolean(false) => eval_expr(&args[2], grid, env),
        CellValue::Error(e) => CellValue::Error(e),
        other => CellValue::Error(LatticeError::type_error("Boolean", variant_name(&other))),
    }
}

fn fold_bool(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    init: bool,
    f: impl Fn(bool, bool) -> bool,
) -> CellValue {
    let mut acc = init;
    for arg in args {
        match eval_expr(arg, grid, env) {
            CellValue::Boolean(b) => acc = f(acc, b),
            CellValue::Error(e) => return CellValue::Error(e),
            other => return CellValue::Error(LatticeError::type_error("Boolean", variant_name(&other))),
        }
    }
    CellValue::Boolean(acc)
}

fn unary_bool(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 1 {
        return CellValue::Error(LatticeError::argument_error("NOT expects one argument"));
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Boolean(b) => CellValue::Boolean(!b),
        CellValue::Error(e) => CellValue::Error(e),
        other => CellValue::Error(LatticeError::type_error("Boolean", variant_name(&other))),
    }
}
