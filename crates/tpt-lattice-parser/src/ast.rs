//! Abstract Syntax Tree for the Lattice Expression Syntax (LES).

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use tpt_lattice_core::{CellId, LatticeError};

/// A complete parsed formula.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Formula {
    /// The root expression of the formula.
    pub body: Expr,
}

/// A LES expression — the central AST node.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Expr {
    /// A literal constant.
    Literal(Literal),
    /// A reference to a single cell (e.g. `A1`).
    CellRef(CellRef),
    /// A reference to a named, undefined identifier (resolved at eval time).
    Name(String),
    /// A unary operation (`-x`, `not x`).
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        expr: Box<Expr>,
    },
    /// A binary operation (`a + b`, `a and b`, ...).
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// A function / aggregate call.
    Call {
        /// Function name (case-insensitive, stored as written).
        name: String,
        /// Call arguments.
        args: Vec<Expr>,
    },
    /// An explicit first-class range (e.g. `RANGE(A1, B10)`).
    Range {
        /// Inclusive start cell.
        start: CellRef,
        /// Inclusive end cell.
        end: CellRef,
    },
    /// `MATCH(value, Ok(x) => body, Err(e) => body)` — explicit error handling.
    Match {
        /// The scrutinee expression.
        scrutinee: Box<Expr>,
        /// One arm per constructor.
        arms: Vec<MatchArm>,
    },
    /// An explicit, strict cast (`NUMBER(x)`, `TEXT(x)`, `BOOLEAN(x)`).
    Cast {
        /// Which primitive to cast to.
        kind: CastKind,
        /// The value to cast.
        expr: Box<Expr>,
    },
}

/// A literal constant.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Literal {
    /// A finite IEEE-754 number.
    Number(f64),
    /// A string literal.
    Text(String),
    /// A boolean literal.
    Boolean(bool),
    /// An inline error literal (rare; primarily used by tooling).
    Error(LatticeError),
}

/// A cell reference, resolved to a packed [`CellId`] plus the original A1 text.
///
/// `abs_col`/`abs_row` record whether the column/row were dollar-prefixed
/// (`$A$1`, `A$1`, `$A1`); they are metadata for copy/fill semantics and do not
/// affect evaluation (a reference always resolves to the same [`CellId`]).
///
/// `sheet` is the optional leading `Sheet!` qualifier for cross-sheet references
/// (e.g. `Sheet2!A1`). When `None`, the reference resolves within the current
/// sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CellRef {
    /// The packed coordinate.
    pub id: CellId,
    /// The original A1 text (e.g. `"$A$1"`), preserved for diagnostics.
    pub a1: String,
    /// Whether the column part was absolute (`$A`).
    pub abs_col: bool,
    /// Whether the row part was absolute (`A$1`).
    pub abs_row: bool,
    /// Optional leading sheet qualifier (e.g. `"Sheet2"` in `Sheet2!A1`).
    pub sheet: Option<String>,
}

impl CellRef {
    /// A re-parseable A1 rendering, including the `Sheet!` qualifier when present.
    pub fn display_a1(&self) -> String {
        match &self.sheet {
            Some(s) => format!("{}!{}", s, self.a1),
            None => self.a1.clone(),
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum UnaryOp {
    /// Arithmetic negation (`-x`).
    Neg,
    /// Logical / bitwise not (`not x`).
    Not,
}

/// Binary operators, ordered by binding precedence in the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BinaryOp {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Remainder (`%`).
    Mod,
    /// Exponentiation (`^`), right-associative.
    Pow,
    /// Equality (`==`).
    Eq,
    /// Inequality (`!=`).
    Ne,
    /// Less than (`<`).
    Lt,
    /// Less than or equal (`<=`).
    Le,
    /// Greater than (`>`).
    Gt,
    /// Greater than or equal (`>=`).
    Ge,
    /// Logical and (`and` / `&&`).
    And,
    /// Logical or (`or` / `||`).
    Or,
    /// String concatenation (`&`).
    Concat,
}

/// The three constructors matched by `MATCH`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MatchPattern {
    /// `Ok(name)` — matches a non-error value, bound to `name`.
    Ok(String),
    /// `Err(name)` — matches an error value, bound to `name`.
    Err(String),
    /// `_` — matches anything.
    Wildcard,
}

/// A single `MATCH` arm.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MatchArm {
    /// The pattern to match.
    pub pattern: MatchPattern,
    /// The expression evaluated when the pattern matches.
    pub body: Expr,
}

/// Explicit cast targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CastKind {
    /// Cast to number (`NUMBER`).
    Number,
    /// Cast to text (`TEXT`).
    Text,
    /// Cast to boolean (`BOOLEAN`).
    Boolean,
}

impl CastKind {
    /// The LES keyword for this cast.
    pub fn keyword(&self) -> &'static str {
        match self {
            CastKind::Number => "NUMBER",
            CastKind::Text => "TEXT",
            CastKind::Boolean => "BOOLEAN",
        }
    }
}

impl core::fmt::Display for CastKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.keyword())
    }
}

impl UnaryOp {
    /// The LES symbol for this operator.
    pub fn symbol(&self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "not",
        }
    }
}

impl BinaryOp {
    /// The LES symbol for this operator.
    pub fn symbol(&self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Pow => "^",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::Concat => "&",
        }
    }
}

/// A fully-parenthesized, unambiguously re-parseable rendering of an [`Expr`].
///
/// The implementation over-parenthesizes every compound node so that
/// `parse(&expr.to_string())` always yields a structurally identical AST — this
/// underpins the parser's round-trip property tests.
impl core::fmt::Display for Expr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Expr::Literal(l) => match l {
                Literal::Number(n) => write!(f, "{n}"),
                Literal::Text(s) => write!(f, "\"{s}\""),
                Literal::Boolean(b) => write!(f, "{b}"),
                Literal::Error(e) => write!(f, "#{e}"),
            },
            Expr::CellRef(c) => f.write_str(&c.display_a1()),
            Expr::Name(n) => f.write_str(n),
            Expr::Unary { op, expr } => write!(f, "({}{})", op.symbol(), expr),
            Expr::Binary { op, left, right } => {
                write!(f, "({} {} {})", left, op.symbol(), right)
            }
            Expr::Call { name, args } => {
                write!(f, "{name}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Expr::Range { start, end } => {
                write!(f, "RANGE({}, {})", start.display_a1(), end.display_a1())
            }
            Expr::Match { scrutinee, arms } => {
                write!(f, "MATCH({scrutinee}")?;
                for arm in arms {
                    write!(f, ", ")?;
                    match &arm.pattern {
                        MatchPattern::Ok(v) => write!(f, "Ok({v}) => {}", arm.body)?,
                        MatchPattern::Err(v) => write!(f, "Err({v}) => {}", arm.body)?,
                        MatchPattern::Wildcard => write!(f, "_ => {}", arm.body)?,
                    }
                }
                write!(f, ")")
            }
            Expr::Cast { kind, expr } => write!(f, "{kind}({expr})"),
        }
    }
}

impl core::fmt::Display for Formula {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "={}", self.body)
    }
}
