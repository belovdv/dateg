use crate::case::Case;

pub mod consumption;
pub mod failures;
pub mod handcrafted;
pub mod random_dense;
pub mod random_memory;
pub mod random_sparse;
pub mod special;
pub mod use_cases;

pub fn all() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(consumption::cases());
    cases.extend(failures::cases());
    cases.extend(handcrafted::cases());
    cases.extend(random_dense::cases());
    cases.extend(random_memory::cases());
    cases.extend(random_sparse::cases());
    cases.extend(special::cases());
    cases.extend(use_cases::cases());
    cases
}
