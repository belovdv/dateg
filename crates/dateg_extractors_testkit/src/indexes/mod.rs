use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

use dateg_extractors::AHashMap;

pub mod container;
pub mod expr;
pub mod mem;

pub fn validate_index<T, E>(
    map: &AHashMap<T, E>,
    root: T,
    children: impl Fn(&E) -> Vec<T>,
    cost: impl Fn(&E) -> usize,
) -> usize
where
    T: Copy + Eq + Hash + Debug,
{
    fn visit<T, E>(
        token: T,
        map: &AHashMap<T, E>,
        children: &impl Fn(&E) -> Vec<T>,
        cost: &impl Fn(&E) -> usize,
        visiting: &mut HashSet<T>,
        done: &mut HashSet<T>,
    ) -> usize
    where
        T: Copy + Eq + Hash + Debug,
    {
        if done.contains(&token) {
            return 0;
        }
        assert!(!visiting.contains(&token), "index cycle at {token:?}");
        visiting.insert(token);
        let value = map
            .get(&token)
            .unwrap_or_else(|| panic!("missing index entry for {token:?}"));
        let mut total = cost(value);
        for child in children(value) {
            total += visit(child, map, children, cost, visiting, done);
        }
        visiting.remove(&token);
        done.insert(token);
        total
    }

    visit(
        root,
        map,
        &children,
        &cost,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
}
