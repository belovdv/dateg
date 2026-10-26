use dateg::TokenOpaque;
use dateg_extractors::dag::{Error, index_dag};

use crate::algorithms::Algorithm;
use crate::memory::{Int, Ints, Memory};

index_dag!(ContainerIndex
    expr: ContainerEnum (datatype Int
        CExprAny (String Ints)
    )
    [Ints]
);

index_dag!(FilteredContainerIndex
    expr: FilteredContainerEnum (datatype Int
        FExprAny (String Ints) {
            |(name, _), eg| ["one", "inc", "mul"].contains(&name.get(eg).as_str()).then_some(1)
        }
    )
    [Ints]
);

pub fn extract_container(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, Error> {
    let extraction = ContainerIndex::extract(m, root, m.expr_v, &algorithm.options(), None)?;
    assert!(extraction.index.expr.contains_key(&root));
    Ok(extraction.cost)
}

pub fn extract_filtered_container(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, Error> {
    let extraction =
        FilteredContainerIndex::extract(m, root, m.expr_v, &algorithm.options(), None)?;
    assert!(extraction.index.expr.contains_key(&root));
    Ok(extraction.cost)
}
