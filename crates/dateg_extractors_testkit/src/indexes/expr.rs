#![allow(clippy::enum_variant_names)]

use dateg::TokenOpaque;
use dateg_extractors::dag::{ExtractError, ExtractStats, index_dag};

use crate::algorithms::Algorithm;
use crate::indexes::validate_index;
use crate::memory::{Int, Memory};

index_dag!(ExprIndex
    expr: ExprEnum (datatype Int
        EOne ()
        EInc (Int)
        EMul (Int Int)
    )
);

index_dag!(FullExprIndex
    expr: FullExprEnum (datatype Int
        FOne ()
        FInc (Int)
        FMul (Int Int)
        FSquare (Int)
    )
);

index_dag!(ZeroExprIndex
    expr: ZeroExprEnum (datatype Int
        ZOne () { |_, _| Some(0) }
        ZInc (Int) { |_, _| Some(0) }
        ZMul (Int Int) { |_, _| Some(0) }
        ZSquare (Int) { |_, _| Some(0) }
    )
);

index_dag!(LoopIndex
    expr: LoopEnum (datatype Int
        LOne ()
        LInc (Int)
        LNeg (Int)
        LSub (Int Int)
    )
);

pub fn extract_expr(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    extract_expr_with(m, root, algorithm, None)
}

pub fn extract_expr_with(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
    stats: Option<&mut ExtractStats>,
) -> Result<usize, ExtractError> {
    let extraction =
        ExprIndex::extract(m, root, (m.one, m.inc, m.mul), &algorithm.options(), stats)?;
    let cost = validate_index(
        &extraction.index.expr,
        root,
        |value| match value {
            ExprEnum::EOne() => vec![],
            ExprEnum::EInc(a) => vec![*a],
            ExprEnum::EMul(a, b) => vec![*a, *b],
        },
        |_| 1,
    );
    assert_eq!(cost, extraction.cost);
    Ok(extraction.cost)
}

pub fn eval_expr(index: &ExprIndex, token: TokenOpaque<Int>) -> usize {
    match index.expr.get(&token).expect("validated value") {
        ExprEnum::EOne() => 1,
        ExprEnum::EInc(a) => eval_expr(index, *a) + 1,
        ExprEnum::EMul(a, b) => eval_expr(index, *a) * eval_expr(index, *b),
    }
}

pub fn extract_expr_eval(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<(usize, usize), ExtractError> {
    let extraction =
        ExprIndex::extract(m, root, (m.one, m.inc, m.mul), &algorithm.options(), None)?;
    let cost = validate_index(
        &extraction.index.expr,
        root,
        |value| match value {
            ExprEnum::EOne() => vec![],
            ExprEnum::EInc(a) => vec![*a],
            ExprEnum::EMul(a, b) => vec![*a, *b],
        },
        |_| 1,
    );
    assert_eq!(cost, extraction.cost);
    Ok((cost, eval_expr(&extraction.index, root)))
}

pub fn extract_expr_string(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<(usize, String), ExtractError> {
    let extraction =
        ExprIndex::extract(m, root, (m.one, m.inc, m.mul), &algorithm.options(), None)?;
    let cost = validate_index(
        &extraction.index.expr,
        root,
        |value| match value {
            ExprEnum::EOne() => vec![],
            ExprEnum::EInc(a) => vec![*a],
            ExprEnum::EMul(a, b) => vec![*a, *b],
        },
        |_| 1,
    );
    assert_eq!(cost, extraction.cost);

    fn render(
        map: &dateg_extractors::AHashMap<TokenOpaque<Int>, ExprEnum>,
        token: TokenOpaque<Int>,
    ) -> String {
        match map.get(&token).expect("validated root") {
            ExprEnum::EOne() => "one".to_string(),
            ExprEnum::EInc(a) => format!("(inc {})", render(map, *a)),
            ExprEnum::EMul(a, b) => {
                let mut a = render(map, *a);
                let mut b = render(map, *b);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                format!("(mul {a} {b})")
            }
        }
    }

    Ok((extraction.cost, render(&extraction.index.expr, root)))
}

pub fn extract_full_expr(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    let extraction = FullExprIndex::extract(
        m,
        root,
        (m.one, m.inc, m.mul, m.square),
        &algorithm.options(),
        None,
    )?;
    let cost = validate_index(
        &extraction.index.expr,
        root,
        |value| match value {
            FullExprEnum::FOne() => vec![],
            FullExprEnum::FInc(a) => vec![*a],
            FullExprEnum::FMul(a, b) => vec![*a, *b],
            FullExprEnum::FSquare(a) => vec![*a],
        },
        |_| 1,
    );
    assert_eq!(cost, extraction.cost);
    Ok(extraction.cost)
}

pub fn extract_zero_expr(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    let extraction = ZeroExprIndex::extract(
        m,
        root,
        (m.one, m.inc, m.mul, m.square),
        &algorithm.options(),
        None,
    )?;
    let cost = validate_index(
        &extraction.index.expr,
        root,
        |value| match value {
            ZeroExprEnum::ZOne() => vec![],
            ZeroExprEnum::ZInc(a) => vec![*a],
            ZeroExprEnum::ZMul(a, b) => vec![*a, *b],
            ZeroExprEnum::ZSquare(a) => vec![*a],
        },
        |_| 0,
    );
    assert_eq!(cost, extraction.cost);
    Ok(extraction.cost)
}

pub fn extract_loop_string(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<(usize, String), ExtractError> {
    let extraction = LoopIndex::extract(
        m,
        root,
        (m.one, m.inc, m.neg, m.sub),
        &algorithm.options(),
        None,
    )?;
    let cost = validate_index(
        &extraction.index.expr,
        root,
        |value| match value {
            LoopEnum::LOne() => vec![],
            LoopEnum::LInc(a) => vec![*a],
            LoopEnum::LNeg(a) => vec![*a],
            LoopEnum::LSub(a, b) => vec![*a, *b],
        },
        |_| 1,
    );
    assert_eq!(cost, extraction.cost);

    fn render(
        map: &dateg_extractors::AHashMap<TokenOpaque<Int>, LoopEnum>,
        token: TokenOpaque<Int>,
    ) -> String {
        match map.get(&token).expect("validated root") {
            LoopEnum::LOne() => "one".to_string(),
            LoopEnum::LInc(a) => format!("(inc {})", render(map, *a)),
            LoopEnum::LNeg(a) => format!("(neg {})", render(map, *a)),
            LoopEnum::LSub(a, b) => format!("(sub {} {})", render(map, *a), render(map, *b)),
        }
    }

    Ok((extraction.cost, render(&extraction.index.expr, root)))
}

pub fn extract_loop(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    extract_loop_string(m, root, algorithm).map(|(cost, _)| cost)
}
