//! Property-based round-trip tests for the parser.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;

use proptest::prelude::*;

use tpt_lattice_core::CellId;

use crate::ast::{BinaryOp, CellRef, Expr, Literal, UnaryOp};
use crate::parse_expr;

fn cellref_strategy() -> impl Strategy<Value = Expr> {
    ("[A-Z]{1,3}", 1u32..1000u32).prop_map(|(letters, n)| {
        let a1 = format!("{letters}{n}");
        Expr::CellRef(CellRef {
            id: CellId::from_a1(&a1),
            a1,
        })
    })
}

fn expr_strategy() -> BoxedStrategy<Expr> {
    let leaf = prop_oneof![
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(|n| Expr::Literal(Literal::Number(n.abs()))),
        cellref_strategy(),
    ];
    leaf.prop_recursive(8, 48, 4, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(l, r)| Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(l),
                right: Box::new(r),
            }),
            (inner.clone(), inner.clone()).prop_map(|(l, r)| Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(l),
                right: Box::new(r),
            }),
            inner.clone().prop_map(|e| Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(e),
            }),
        ]
    })
    .boxed()
}

proptest! {
    #[test]
    fn roundtrips(e in expr_strategy()) {
        let printed = e.to_string();
        let reparsed = parse_expr(&printed).expect("printed expression must parse");
        prop_assert_eq!(e, reparsed);
    }

    #[test]
    fn never_panics(s in "\\PC*") {
        // Parsing arbitrary input must return Ok/Err, never panic.
        let _ = parse_expr(&s);
    }
}
