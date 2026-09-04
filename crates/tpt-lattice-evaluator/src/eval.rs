//! Evaluation of a parsed [`Expr`] against a [`GridState`].

use std::collections::HashMap;

use tpt_lattice_core::{
    format_serial_date, serial_from_ymd, ymd_from_serial, CellId, CellValue, GridState,
    LatticeError,
};
use tpt_lattice_parser::ast::{BinaryOp, CastKind, Expr, Literal, MatchPattern, UnaryOp};

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
        Expr::CellRef(c) => {
            if let Some(sheet) = &c.sheet {
                match grid.get_sheet_cell(sheet, c.id) {
                    Some(v) => v.sanitize(),
                    None => CellValue::Error(LatticeError::ref_error(format!(
                        "no such sheet '{sheet}'"
                    ))),
                }
            } else {
                grid.get_cell(c.id).sanitize()
            }
        }
        Expr::Name(n) => match env.get(n) {
            Some(v) => v.clone(),
            None => match grid.get_named(n) {
                Some(v) => v,
                None => CellValue::Error(LatticeError::name_error(n.clone())),
            },
        },
        Expr::Unary { op, expr } => eval_unary(*op, expr, grid, env),
        Expr::Binary { op, left, right } => eval_binary(*op, left, right, grid, env),
        Expr::Call { name, args } => call_function(name, args, grid, env),
        Expr::Range { .. } => CellValue::Error(LatticeError::argument_error(
            "RANGE must be passed as an argument to a function (e.g. SUM, INDEX, VLOOKUP)",
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
        BinaryOp::Add => match eval_date_binary(BinaryOp::Add, &l, &r) {
            Some(v) => v,
            None => arith(l, r, |a, b| a + b),
        },
        BinaryOp::Sub => match eval_date_binary(BinaryOp::Sub, &l, &r) {
            Some(v) => v,
            None => arith(l, r, |a, b| a - b),
        },
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
        BinaryOp::Concat => concat_values(l, r),
    }
}

fn eval_date_binary(op: BinaryOp, l: &CellValue, r: &CellValue) -> Option<CellValue> {
    use CellValue::*;
    match (l, r) {
        (Date(a), Date(b)) if matches!(op, BinaryOp::Sub) => Some(Number(a - b)),
        (Date(a), Number(b)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => Some(Date(
            if matches!(op, BinaryOp::Add) { a + b } else { a - b },
        )),
        (Number(a), Date(b)) if matches!(op, BinaryOp::Add) => Some(Date(a + b)),
        _ => None,
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
        (CellValue::Number(a), CellValue::Number(b)) => {
            bool_of(pred(a.partial_cmp(b).unwrap_or(Ordering::Equal)))
        }
        (CellValue::Text(a), CellValue::Text(b)) => bool_of(pred(a.cmp(b))),
        (CellValue::Boolean(a), CellValue::Boolean(b)) => bool_of(pred(a.cmp(b))),
        (CellValue::Date(a), CellValue::Date(b)) => {
            bool_of(pred(a.partial_cmp(b).unwrap_or(Ordering::Equal)))
        }
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
        CellValue::Date(_) => "Date",
        CellValue::List(_) => "List",
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
            CellValue::Date(d) => CellValue::Number(d),
            CellValue::List(_) => CellValue::Error(LatticeError::type_error("Number", "List")),
            CellValue::Empty => CellValue::Error(LatticeError::type_error("Number", "Empty")),
        },
        CastKind::Text => match v {
            CellValue::Text(s) => CellValue::Text(s),
            CellValue::Number(n) => CellValue::Text(n.to_string()),
            CellValue::Boolean(b) => CellValue::Text(b.to_string()),
            CellValue::Error(e) => CellValue::Text(format!("#{e}")),
            CellValue::Date(d) => CellValue::Text(format_serial_date(d)),
            CellValue::List(items) => {
                CellValue::Text(items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
            }
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
            CellValue::Date(_) => CellValue::Boolean(true),
            CellValue::List(_) => CellValue::Error(LatticeError::type_error("Boolean", "List")),
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
        // --- Predicates -----------------------------------------------------
        "ISBLANK" => predicate(args, grid, env, |v| v.is_empty()),
        "ISERROR" => predicate(args, grid, env, |v| v.is_error()),
        "ISNUMBER" => predicate(args, grid, env, |v| matches!(v, CellValue::Number(_))),
        "ISTEXT" => predicate(args, grid, env, |v| matches!(v, CellValue::Text(_))),
        "ISNA" => predicate(args, grid, env, |v| {
            matches!(v, CellValue::Error(LatticeError::NA))
        }),
        // --- Error handling wrappers ----------------------------------------
        "IFERROR" => iferror_fn(args, grid, env),
        "IFNA" => ifna_fn(args, grid, env),
        // --- String functions ----------------------------------------------
        "UPPER" => text_transform(args, grid, env, |s| s.to_uppercase(), "UPPER"),
        "LOWER" => text_transform(args, grid, env, |s| s.to_lowercase(), "LOWER"),
        "TRIM" => text_transform(
            args,
            grid,
            env,
            |s| s.split_whitespace().collect::<Vec<_>>().join(" "),
            "TRIM",
        ),
        "LEFT" => left_right(args, grid, env, true),
        "RIGHT" => left_right(args, grid, env, false),
        "MID" => mid(args, grid, env),
        "FIND" => find(args, grid, env),
        "SUBSTITUTE" => substitute(args, grid, env),
        "REPLACE" => replace(args, grid, env),
        // --- Conditional aggregates -----------------------------------------
        "COUNTIF" => countif(args, grid, env),
        "SUMIF" => sumif(args, grid, env),
        "AVERAGEIF" => averageif(args, grid, env),
        "SUMIFS" => sumifs(args, grid, env),
        // --- Lookups --------------------------------------------------------
        "VLOOKUP" => vlookup(args, grid, env, false),
        "HLOOKUP" => vlookup(args, grid, env, true),
        "INDEX" => index_fn(args, grid, env),
        "XLOOKUP" => xlookup(args, grid, env),
        // --- Statistics -----------------------------------------------------
        "MEDIAN" => median(args, grid, env),
        "VAR" => sample_var(args, grid, env),
        "STDEV" => stdev(args, grid, env),
        "MODE" => mode_fn(args, grid, env),
        "RANK" => rank_fn(args, grid, env),
        "PERCENTILE" => percentile(args, grid, env),
        // --- Date & time ----------------------------------------------------
        "DATE" => date_fn(args, grid, env),
        "TODAY" => today_fn(args),
        "NOW" => now_fn(args),
        "YEAR" => year_fn(args, grid, env),
        "MONTH" => month_fn(args, grid, env),
        "DAY" => day_fn(args, grid, env),
        "DATEDIF" => datedif(args, grid, env),
        other => {
            // Not a built-in: evaluate every argument, then consult any
            // registered external (user-defined) function. If none is registered
            // under `other`, fall back to a `#NAME?` error.
            let arg_vals: Vec<CellValue> = args.iter().map(|a| eval_expr(a, grid, env)).collect();
            match grid.call_external(other, &arg_vals) {
                Some(v) => v.sanitize(),
                None => CellValue::Error(LatticeError::name_error(other)),
            }
        }
    }
}

/// Collect numeric values referenced by the argument expressions.
fn collect_numbers(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    strict: bool,
) -> Result<Vec<f64>, LatticeError> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Expr::Range { start, end } => {
                let mut ids = Vec::new();
                crate::dag::expand_range(start, end, &mut ids)?;
                for id in ids {
                    match grid.get_cell(id) {
                        CellValue::Number(n) => out.push(n),
                        CellValue::Empty => {}
                        CellValue::Error(e) => return Err(e),
                        other if strict => {
                            return Err(LatticeError::type_error(
                                "Number",
                                variant_name(&other),
                            ))
                        }
                        _ => {}
                    }
                }
            }
            Expr::CellRef(c) => match grid.get_cell(c.id) {
                CellValue::Number(n) => out.push(n),
                CellValue::Empty => {}
                CellValue::Error(e) => return Err(e),
                other if strict => {
                    return Err(LatticeError::type_error("Number", variant_name(&other)))
                }
                _ => {}
            },
            other => match eval_expr(other, grid, env) {
                CellValue::Number(n) => out.push(n),
                CellValue::Error(e) => return Err(e),
                v if strict => {
                    return Err(LatticeError::type_error("Number", variant_name(&v)))
                }
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
        return CellValue::Error(LatticeError::argument_error(format!(
            "{name} expects at least one argument"
        )));
    }
    match collect_numbers(args, grid, env, true) {
        Ok(nums) => CellValue::Number(nums.into_iter().fold(init, &f)),
        Err(e) => CellValue::Error(e),
    }
}

fn min_max(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    is_min: bool,
) -> CellValue {
    match collect_numbers(args, grid, env, true) {
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
    match collect_numbers(args, grid, env, true) {
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
    match collect_numbers(args, grid, env, false) {
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
        return CellValue::Error(LatticeError::argument_error(format!(
            "{name} expects one argument"
        )));
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
        return CellValue::Error(LatticeError::argument_error(format!(
            "{name} expects two arguments"
        )));
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
        return CellValue::Error(LatticeError::argument_error(
            "ROUND expects 1 or 2 arguments",
        ));
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
            other => {
                return CellValue::Error(LatticeError::type_error("Number", variant_name(&other)))
            }
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
            CellValue::Date(d) => out.push_str(&format_serial_date(d)),
            CellValue::List(_) => return CellValue::Error(LatticeError::type_error("Text", "List")),
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
    if args.len() != 2 && args.len() != 3 {
        return CellValue::Error(LatticeError::argument_error(
            "IF expects (cond, then[, else])",
        ));
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Boolean(true) => eval_expr(&args[1], grid, env),
        CellValue::Boolean(false) => {
            if args.len() == 3 {
                eval_expr(&args[2], grid, env)
            } else {
                CellValue::Empty
            }
        }
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
            other => {
                return CellValue::Error(LatticeError::type_error("Boolean", variant_name(&other)))
            }
        }
    }
    CellValue::Boolean(acc)
}

fn unary_bool(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 1 {
        return CellValue::Error(LatticeError::argument_error("NOT expects one argument"));
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Boolean(b) => CellValue::Boolean(!b),
        CellValue::Error(e) => CellValue::Error(e),
        other => CellValue::Error(LatticeError::type_error("Boolean", variant_name(&other))),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for the extended function library (Phase 8)
// ---------------------------------------------------------------------------

/// Render any value as the text a string function would operate on.
fn to_text(v: &CellValue) -> String {
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Number(n) => n.to_string(),
        CellValue::Boolean(b) => b.to_string(),
        CellValue::Date(s) => format_serial_date(*s),
        CellValue::List(items) => items
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        CellValue::Empty => String::new(),
        CellValue::Error(e) => format!("#{e}"),
    }
}

fn concat_values(l: CellValue, r: CellValue) -> CellValue {
    match (&l, &r) {
        (CellValue::Error(e), _) | (_, CellValue::Error(e)) => CellValue::Error(e.clone()),
        _ => CellValue::Text(to_text(&l) + &to_text(&r)),
    }
}

/// Evaluate `arg` to a text value, coercing numbers/booleans (as Excel does for
/// string functions). Errors propagate; values that cannot be reasonably coerced
/// become a [`LatticeError::TypeError`].
fn eval_text(
    arg: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    match eval_expr(arg, grid, env) {
        CellValue::Text(s) => CellValue::Text(s),
        CellValue::Number(n) => CellValue::Text(n.to_string()),
        CellValue::Boolean(b) => CellValue::Text(b.to_string()),
        CellValue::Empty => CellValue::Text(String::new()),
        CellValue::Date(d) => CellValue::Text(format_serial_date(d)),
        CellValue::List(items) => {
            CellValue::Text(items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
        }
        e @ CellValue::Error(_) => e,
    }
}

/// Evaluate `arg` to a non-negative integer index (used by LEFT/RIGHT/MID/...).
fn eval_index(
    arg: &Expr,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> Result<usize, CellValue> {
    match eval_expr(arg, grid, env) {
        CellValue::Number(n) if n.is_finite() && n >= 0.0 => Ok(n as usize),
        CellValue::Error(e) => Err(CellValue::Error(e)),
        other => Err(CellValue::Error(LatticeError::type_error(
            "Number",
            variant_name(&other),
        ))),
    }
}

fn count_error(name: &str) -> CellValue {
    CellValue::Error(LatticeError::argument_error(format!(
        "wrong number of arguments for {name}"
    )))
}

fn predicate(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    test: impl Fn(&CellValue) -> bool,
) -> CellValue {
    if args.len() != 1 {
        return count_error("predicate");
    }
    let v = eval_expr(&args[0], grid, env);
    CellValue::Boolean(test(&v))
}

fn iferror_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 2 {
        return count_error("IFERROR");
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Error(_) => eval_expr(&args[1], grid, env),
        other => other,
    }
}

fn ifna_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 2 {
        return count_error("IFNA");
    }
    match eval_expr(&args[0], grid, env) {
        CellValue::Error(LatticeError::NA) => eval_expr(&args[1], grid, env),
        other => other,
    }
}

fn text_transform(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    f: impl Fn(&str) -> String,
    name: &str,
) -> CellValue {
    if args.len() != 1 {
        return count_error(name);
    }
    match eval_text(&args[0], grid, env) {
        CellValue::Text(s) => CellValue::Text(f(&s)),
        e => e,
    }
}

fn left_right(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    from_left: bool,
) -> CellValue {
    let n = args.len();
    if !(1..=2).contains(&n) {
        return count_error(if from_left { "LEFT" } else { "RIGHT" });
    }
    let s = match eval_text(&args[0], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let count = if n == 2 {
        match eval_index(&args[1], grid, env) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        1
    };
    let chars: Vec<char> = s.chars().collect();
    let count = count.min(chars.len());
    let piece: String = if from_left {
        chars.iter().take(count).collect()
    } else {
        chars.iter().skip(chars.len().saturating_sub(count)).collect()
    };
    CellValue::Text(piece)
}

fn mid(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if !(2..=3).contains(&args.len()) {
        return count_error("MID");
    }
    let s = match eval_text(&args[0], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let start = match eval_index(&args[1], grid, env) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let len = if args.len() == 3 {
        match eval_index(&args[2], grid, env) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        usize::MAX
    };
    if start == 0 {
        return CellValue::Text(String::new());
    }
    let chars: Vec<char> = s.chars().collect();
    let start_idx = start - 1;
    if start_idx >= chars.len() {
        return CellValue::Text(String::new());
    }
    let end = (start_idx + len).min(chars.len());
    CellValue::Text(chars[start_idx..end].iter().collect())
}

fn find(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if !(2..=3).contains(&args.len()) {
        return count_error("FIND");
    }
    let needle = match eval_text(&args[0], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let haystack = match eval_text(&args[1], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let start = if args.len() == 3 {
        match eval_index(&args[2], grid, env) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        1
    };
    if start == 0 {
        return CellValue::Error(LatticeError::argument_error(
            "FIND start position must be >= 1",
        ));
    }
    let h: Vec<char> = haystack.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return CellValue::Number(1.0);
    }
    let sidx = start - 1;
    if sidx >= h.len() {
        return CellValue::Error(LatticeError::na());
    }
    let max = h.len() - n.len();
    for i in sidx..=max {
        if h[i..i + n.len()] == n[..] {
            return CellValue::Number((i + 1) as f64);
        }
    }
    CellValue::Error(LatticeError::na())
}

fn substitute(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if !(3..=4).contains(&args.len()) {
        return count_error("SUBSTITUTE");
    }
    let text = match eval_text(&args[0], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let old = match eval_text(&args[1], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let new = match eval_text(&args[2], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let nth = if args.len() == 4 {
        match eval_index(&args[3], grid, env) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        0
    };
    if old.is_empty() {
        return CellValue::Text(text);
    }
    if nth == 0 {
        return CellValue::Text(text.replace(&old, &new));
    }
    let mut out = String::new();
    let mut count = 0;
    let mut rest = text.as_str();
    while let Some(pos) = rest.find(&old) {
        count += 1;
        if count == nth {
            out.push_str(&rest[..pos]);
            out.push_str(&new);
            out.push_str(&rest[pos + old.len()..]);
            return CellValue::Text(out);
        }
        out.push_str(&rest[..pos + old.len()]);
        rest = &rest[pos + old.len()..];
    }
    CellValue::Text(out)
}

fn replace(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 4 {
        return count_error("REPLACE");
    }
    let text = match eval_text(&args[0], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let start = match eval_index(&args[1], grid, env) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let len = match eval_index(&args[2], grid, env) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let new = match eval_text(&args[3], grid, env) {
        CellValue::Text(s) => s,
        e => return e,
    };
    let chars: Vec<char> = text.chars().collect();
    let s = if start == 0 { 0 } else { start - 1 };
    let e = (s + len).min(chars.len());
    let mut out: String = chars[..s].iter().collect();
    out.push_str(&new);
    out.extend(chars[e..].iter());
    CellValue::Text(out)
}

/// Expand a `RANGE(...)` argument into the list of cell values it covers.
fn eval_range(arg: &Expr, grid: &dyn GridState) -> Result<Vec<CellValue>, CellValue> {
    match arg {
        Expr::Range { start, end } => {
            let mut ids = Vec::new();
            if let Err(e) = crate::dag::expand_range(start, end, &mut ids) {
                return Err(CellValue::Error(e));
            }
            Ok(ids.iter().map(|id| grid.get_cell(*id).sanitize()).collect())
        }
        _ => Err(CellValue::Error(LatticeError::argument_error(
            "expected a RANGE(...) argument",
        ))),
    }
}

/// Expand a `RANGE(...)` and return its flat (row-major) cell-id list plus the
/// `(cols, rows)` dimensions of the rectangle.
fn expand_range_meta(
    arg: &Expr,
    _grid: &dyn GridState,
) -> Result<(Vec<CellId>, usize, usize), CellValue> {
    match arg {
        Expr::Range { start, end } => {
            let (sc, sr) = start.id.to_rc();
            let (ec, er) = end.id.to_rc();
            let (c0, c1) = (sc.min(ec), sc.max(ec));
            let (r0, r1) = (sr.min(er), sr.max(er));
            let cols = (c1 - c0 + 1) as usize;
            let rows = (r1 - r0 + 1) as usize;
            let mut ids = Vec::new();
            if let Err(e) = crate::dag::expand_range(start, end, &mut ids) {
                return Err(CellValue::Error(e));
            }
            Ok((ids, cols, rows))
        }
        _ => Err(CellValue::Error(LatticeError::argument_error(
            "expected a RANGE(...) argument",
        ))),
    }
}

// --- Conditional-aggregate criterion matching ---------------------------------

enum CmpOp {
    Gt,
    Lt,
    Ge,
    Le,
}

enum Criterion {
    Exact(CellValue),
    Ne(CellValue),
    Cmp(CmpOp, CellValue),
}

/// Interpret a criterion expression (typically a text literal such as `">5"` or
/// a bare value as an exact match).
fn parse_criterion(cond: &CellValue) -> Criterion {
    if let CellValue::Text(s) = cond {
        if let Some(rest) = s.strip_prefix("<>") {
            return Criterion::Ne(criterion_value(rest));
        }
        if let Some(rest) = s.strip_prefix(">=") {
            return Criterion::Cmp(CmpOp::Ge, criterion_value(rest));
        }
        if let Some(rest) = s.strip_prefix("<=") {
            return Criterion::Cmp(CmpOp::Le, criterion_value(rest));
        }
        if let Some(rest) = s.strip_prefix(">") {
            return Criterion::Cmp(CmpOp::Gt, criterion_value(rest));
        }
        if let Some(rest) = s.strip_prefix("<") {
            return Criterion::Cmp(CmpOp::Lt, criterion_value(rest));
        }
        if let Some(rest) = s.strip_prefix("=") {
            return Criterion::Exact(criterion_value(rest));
        }
    }
    Criterion::Exact(cond.clone())
}

fn criterion_value(s: &str) -> CellValue {
    match s.trim().parse::<f64>() {
        Ok(n) if n.is_finite() => CellValue::Number(n),
        _ => CellValue::Text(s.to_string()),
    }
}

fn matches_criterion(value: &CellValue, crit: &Criterion) -> bool {
    match crit {
        Criterion::Exact(c) => value == c,
        Criterion::Ne(c) => value != c,
        Criterion::Cmp(op, c) => {
            let ord = match (value, c) {
                (CellValue::Number(a), CellValue::Number(b)) => a.partial_cmp(b),
                (CellValue::Text(a), CellValue::Text(b)) => Some(a.cmp(b)),
                _ => None,
            };
            matches!(
                (ord, op),
                (Some(std::cmp::Ordering::Less), CmpOp::Lt | CmpOp::Le)
                    | (Some(std::cmp::Ordering::Greater), CmpOp::Gt | CmpOp::Le)
                    | (Some(std::cmp::Ordering::Equal), CmpOp::Ge | CmpOp::Le)
            )
        }
    }
}

fn countif(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 2 {
        return count_error("COUNTIF");
    }
    let values = match eval_range(&args[0], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let cond = eval_expr(&args[1], grid, env);
    let crit = parse_criterion(&cond);
    let n = values.iter().filter(|v| matches_criterion(v, &crit)).count();
    CellValue::Number(n as f64)
}

fn sumif(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() != 2 && args.len() != 3 {
        return count_error("SUMIF");
    }
    let range_vals = match eval_range(&args[0], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let cond = eval_expr(&args[1], grid, env);
    let crit = parse_criterion(&cond);
    let sum_vals: Vec<CellValue> = if args.len() == 3 {
        match eval_range(&args[2], grid) {
            Ok(v) => v,
            Err(e) => return e,
        }
    } else {
        range_vals.clone()
    };
    let mut sum = 0.0;
    for (i, v) in range_vals.iter().enumerate() {
        if matches_criterion(v, &crit) {
            if let CellValue::Number(n) = sum_vals.get(i).cloned().unwrap_or(CellValue::Empty) {
                sum += n;
            }
        }
    }
    CellValue::Number(sum)
}

fn averageif(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 2 && args.len() != 3 {
        return count_error("AVERAGEIF");
    }
    let range_vals = match eval_range(&args[0], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let cond = eval_expr(&args[1], grid, env);
    let crit = parse_criterion(&cond);
    let avg_vals: Vec<CellValue> = if args.len() == 3 {
        match eval_range(&args[2], grid) {
            Ok(v) => v,
            Err(e) => return e,
        }
    } else {
        range_vals.clone()
    };
    let mut sum = 0.0;
    let mut count = 0;
    for (i, v) in range_vals.iter().enumerate() {
        if matches_criterion(v, &crit) {
            if let CellValue::Number(n) = avg_vals.get(i).cloned().unwrap_or(CellValue::Empty) {
                sum += n;
                count += 1;
            }
        }
    }
    if count == 0 {
        return CellValue::Error(LatticeError::DivByZero);
    }
    CellValue::Number(sum / count as f64)
}

fn sumifs(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if args.len() < 3 || args.len() % 2 == 0 {
        return count_error("SUMIFS");
    }
    let sum_vals = match eval_range(&args[0], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let n = sum_vals.len();
    let mut mask = vec![true; n];
    let mut i = 1;
    while i + 1 < args.len() {
        let rv = match eval_range(&args[i], grid) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let cond = eval_expr(&args[i + 1], grid, env);
        let crit = parse_criterion(&cond);
        for (j, v) in rv.iter().enumerate() {
            if j >= n {
                continue;
            }
            if !matches_criterion(v, &crit) {
                mask[j] = false;
            }
        }
        i += 2;
    }
    let sum: f64 = sum_vals
        .iter()
        .enumerate()
        .filter(|(j, _)| mask[*j])
        .filter_map(|(_, v)| if let CellValue::Number(n) = v { Some(*n) } else { None })
        .sum();
    CellValue::Number(sum)
}

// --- Lookups ------------------------------------------------------------------

fn index_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if !(2..=3).contains(&args.len()) {
        return count_error("INDEX");
    }
    let (ids, cols, rows) = match expand_range_meta(&args[0], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let row = match eval_index(&args[1], grid, env) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let col = if args.len() == 3 {
        match eval_index(&args[2], grid, env) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        1
    };
    if row == 0 || col == 0 || row > rows || col > cols {
        return CellValue::Error(LatticeError::ref_error("INDEX position out of range"));
    }
    let idx = (row - 1) * cols + (col - 1);
    grid.get_cell(ids[idx]).sanitize()
}

/// `VLOOKUP` (horizontal=false) and `HLOOKUP` (horizontal=true). Searches the
/// first column/row of the table for an exact match and returns the value at
/// the requested column/row index.
fn vlookup(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
    horizontal: bool,
) -> CellValue {
    let name = if horizontal { "HLOOKUP" } else { "VLOOKUP" };
    if !(3..=4).contains(&args.len()) {
        return count_error(name);
    }
    let key = eval_expr(&args[0], grid, env);
    let (ids, cols, rows) = match expand_range_meta(&args[1], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let idx = match eval_index(&args[2], grid, env) {
        Ok(c) => c,
        Err(e) => return e,
    };
    if idx == 0 {
        return CellValue::Error(LatticeError::ref_error(format!(
            "{name} index must be >= 1"
        )));
    }
    if horizontal {
        if idx > rows {
            return CellValue::Error(LatticeError::ref_error(format!(
                "{name} index out of range"
            )));
        }
        for c in 0..cols {
            let search_pos = c; // first row
            if grid.get_cell(ids[search_pos]).sanitize() == key {
                let ret_pos = (idx - 1) * cols + c;
                return grid.get_cell(ids[ret_pos]).sanitize();
            }
        }
    } else {
        if idx > cols {
            return CellValue::Error(LatticeError::ref_error(format!(
                "{name} index out of range"
            )));
        }
        for r in 0..rows {
            let search_pos = r * cols; // first column
            if grid.get_cell(ids[search_pos]).sanitize() == key {
                let ret_pos = r * cols + (idx - 1);
                return grid.get_cell(ids[ret_pos]).sanitize();
            }
        }
    }
    CellValue::Error(LatticeError::na())
}

fn xlookup(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if !(3..=6).contains(&args.len()) {
        return count_error("XLOOKUP");
    }
    let key = eval_expr(&args[0], grid, env);
    let (lu, _, _) = match expand_range_meta(&args[1], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (ret, _, _) = match expand_range_meta(&args[2], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    for (k, id) in lu.iter().enumerate() {
        if grid.get_cell(*id).sanitize() == key {
            if k < ret.len() {
                return grid.get_cell(ret[k]).sanitize();
            }
            return CellValue::Error(LatticeError::ref_error(
                "XLOOKUP return index out of range",
            ));
        }
    }
    if args.len() >= 4 {
        return eval_expr(&args[3], grid, env);
    }
    CellValue::Error(LatticeError::na())
}

// --- Statistics ----------------------------------------------------------------

fn median(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    match collect_numbers(args, grid, env, true) {
        Ok(mut nums) => {
            if nums.is_empty() {
                return CellValue::Empty;
            }
            nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = nums.len();
            if n % 2 == 1 {
                CellValue::Number(nums[n / 2])
            } else {
                CellValue::Number((nums[n / 2 - 1] + nums[n / 2]) / 2.0)
            }
        }
        Err(e) => CellValue::Error(e),
    }
}

fn sample_var(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    match collect_numbers(args, grid, env, true) {
        Ok(nums) => {
            let n = nums.len() as f64;
            if n < 2.0 {
                return CellValue::Error(LatticeError::DivByZero);
            }
            let mean = nums.iter().sum::<f64>() / n;
            let var = nums.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
            CellValue::Number(var)
        }
        Err(e) => CellValue::Error(e),
    }
}

fn stdev(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    match sample_var(args, grid, env) {
        CellValue::Number(v) => CellValue::Number(v.sqrt()),
        other => other,
    }
}

fn mode_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    match collect_numbers(args, grid, env, true) {
        Ok(nums) => {
            if nums.is_empty() {
                return CellValue::Error(LatticeError::na());
            }
            let mut best: Option<f64> = None;
            let mut best_count = 0;
            for &x in &nums {
                let c = nums.iter().filter(|&&y| y == x).count();
                if c > best_count {
                    best_count = c;
                    best = Some(x);
                }
            }
            if best_count <= 1 {
                return CellValue::Error(LatticeError::na());
            }
            CellValue::Number(best.unwrap())
        }
        Err(e) => CellValue::Error(e),
    }
}

fn rank_fn(args: &[Expr], grid: &dyn GridState, env: &mut HashMap<String, CellValue>) -> CellValue {
    if !(2..=3).contains(&args.len()) {
        return count_error("RANK");
    }
    let value = eval_expr(&args[0], grid, env);
    let list = match eval_range(&args[1], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let order = if args.len() == 3 {
        match eval_index(&args[2], grid, env) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        0
    };
    let target = match value {
        CellValue::Number(n) => n,
        CellValue::Error(e) => return CellValue::Error(e),
        other => return CellValue::Error(LatticeError::type_error("Number", variant_name(&other))),
    };
    let numbers: Vec<f64> = list
        .iter()
        .filter_map(|v| if let CellValue::Number(n) = v { Some(*n) } else { None })
        .collect();
    let rank = if order == 0 {
        1 + numbers.iter().filter(|&&x| x > target).count()
    } else {
        1 + numbers.iter().filter(|&&x| x < target).count()
    };
    CellValue::Number(rank as f64)
}

fn percentile(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 2 {
        return count_error("PERCENTILE");
    }
    let list = match eval_range(&args[0], grid) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let k = match eval_expr(&args[1], grid, env) {
        CellValue::Number(n) => n,
        CellValue::Error(e) => return CellValue::Error(e),
        other => return CellValue::Error(LatticeError::type_error("Number", variant_name(&other))),
    };
    if !(0.0..=1.0).contains(&k) {
        return CellValue::Error(LatticeError::argument_error(
            "PERCENTILE k must be in [0, 1]",
        ));
    }
    let mut nums: Vec<f64> = list
        .iter()
        .filter_map(|v| if let CellValue::Number(n) = v { Some(*n) } else { None })
        .collect();
    if nums.is_empty() {
        return CellValue::Error(LatticeError::argument_error(
            "PERCENTILE needs at least one number",
        ));
    }
    nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = (nums.len() - 1) as f64;
    let pos = k * n;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return CellValue::Number(nums[lo]);
    }
    let frac = pos - lo as f64;
    CellValue::Number(nums[lo] + (nums[hi] - nums[lo]) * frac)
}

// --- Date & time -------------------------------------------------------------

fn serial_today() -> f64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as f64)
        .unwrap_or(0.0);
    let days = (secs / 86_400.0).floor() as i64;
    (days + tpt_lattice_core::EXCEL_EPOCH_OFFSET) as f64
}

fn serial_now() -> f64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let days = (secs / 86_400.0).floor() as i64;
    let frac = secs / 86_400.0 - days as f64;
    (days + tpt_lattice_core::EXCEL_EPOCH_OFFSET) as f64 + frac
}

/// Interpret `v` as an Excel date serial (a [`CellValue::Date`] or a plain
/// number standing in for a serial). Errors and other types are rejected.
fn as_serial(v: &CellValue) -> Result<f64, CellValue> {
    match v {
        CellValue::Date(s) => Ok(*s),
        CellValue::Number(n) => Ok(*n),
        CellValue::Error(e) => Err(CellValue::Error(e.clone())),
        CellValue::Empty => Err(CellValue::Error(LatticeError::type_error(
            "Date",
            "Empty",
        ))),
        other => Err(CellValue::Error(LatticeError::type_error(
            "Date",
            variant_name(other),
        ))),
    }
}

/// Read an integer-valued argument (used by `DATE`, `YEAR`, ...).
fn eval_int_arg(
    args: &[Expr],
    i: usize,
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> Result<i64, CellValue> {
    match eval_expr(&args[i], grid, env) {
        CellValue::Number(n) if n.is_finite() => Ok(n as i64),
        CellValue::Error(e) => Err(CellValue::Error(e)),
        other => Err(CellValue::Error(LatticeError::type_error(
            "Number",
            variant_name(&other),
        ))),
    }
}

fn date_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 3 {
        return count_error("DATE");
    }
    let y = match eval_int_arg(args, 0, grid, env) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let m = match eval_int_arg(args, 1, grid, env) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let d = match eval_int_arg(args, 2, grid, env) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 {
        return CellValue::Error(LatticeError::argument_error(
            "DATE: year/month/day out of range",
        ));
    }
    let days_in_month = days_in_month(y as i32, m as u32);
    if d as u32 > days_in_month {
        return CellValue::Error(LatticeError::argument_error(
            "DATE: day out of range for the given month",
        ));
    }
    CellValue::Date(serial_from_ymd(y as i32, m as u32, d as u32))
}

fn today_fn(args: &[Expr]) -> CellValue {
    if !args.is_empty() {
        return count_error("TODAY");
    }
    CellValue::Date(serial_today())
}

fn now_fn(args: &[Expr]) -> CellValue {
    if !args.is_empty() {
        return count_error("NOW");
    }
    CellValue::Date(serial_now())
}

fn year_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 1 {
        return count_error("YEAR");
    }
    match as_serial(&eval_expr(&args[0], grid, env)) {
        Ok(s) => {
            let (y, _, _) = ymd_from_serial(s);
            CellValue::Number(y as f64)
        }
        Err(e) => e,
    }
}

fn month_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 1 {
        return count_error("MONTH");
    }
    match as_serial(&eval_expr(&args[0], grid, env)) {
        Ok(s) => {
            let (_, m, _) = ymd_from_serial(s);
            CellValue::Number(m as f64)
        }
        Err(e) => e,
    }
}

fn day_fn(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 1 {
        return count_error("DAY");
    }
    match as_serial(&eval_expr(&args[0], grid, env)) {
        Ok(s) => {
            let (_, _, d) = ymd_from_serial(s);
            CellValue::Number(d as f64)
        }
        Err(e) => e,
    }
}



fn datedif(
    args: &[Expr],
    grid: &dyn GridState,
    env: &mut HashMap<String, CellValue>,
) -> CellValue {
    if args.len() != 3 {
        return count_error("DATEDIF");
    }
    let start = match as_serial(&eval_expr(&args[0], grid, env)) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let end = match as_serial(&eval_expr(&args[1], grid, env)) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if end < start {
        return CellValue::Error(LatticeError::argument_error(
            "DATEDIF: end date must not precede start date",
        ));
    }
    let unit = match eval_text(&args[2], grid, env) {
        CellValue::Text(s) => s.to_uppercase(),
        CellValue::Error(e) => return CellValue::Error(e),
        other => {
            return CellValue::Error(LatticeError::type_error("Text", variant_name(&other)))
        }
    };
    let (sy, sm, sd) = ymd_from_serial(start);
    let (ey, em, ed) = ymd_from_serial(end);
    let result: i64 = match unit.as_str() {
        "Y" => {
            let mut y = (ey - sy) as i64;
            if (em, ed) < (sm, sd) {
                y -= 1;
            }
            y
        }
        "M" => {
            let mut m = (ey - sy) as i64 * 12 + em as i64 - sm as i64;
            if ed < sd {
                m -= 1;
            }
            m
        }
        "D" => end.floor() as i64 - start.floor() as i64,
        "YM" => {
            let mut m = em as i64 - sm as i64;
            if ed < sd {
                m -= 1;
            }
            if m < 0 {
                m += 12;
            }
            m
        }
        "YD" => {
            let anchor = serial_from_ymd(ey, sm, sd);
            let d = end.floor() as i64 - anchor.floor() as i64;
            if d < 0 {
                let prev = serial_from_ymd(ey - 1, sm, sd);
                end.floor() as i64 - prev.floor() as i64
            } else {
                d
            }
        }
        "MD" => {
            let mut d = ed as i64 - sd as i64;
            if d < 0 {
                let (py, pm) = prev_month(ey, em);
                d += days_in_month(py, pm) as i64;
            }
            d
        }
        _ => {
            return CellValue::Error(LatticeError::argument_error(
                "DATEDIF: unit must be Y, M, D, YM, YD, or MD",
            ))
        }
    };
    CellValue::Number(result as f64)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn prev_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}
