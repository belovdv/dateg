/// Public solver error.
#[derive(Debug)]
pub enum Error {
    RootNotCanonical,
    /// The root is not present in the selected constructor tables.
    RootMissing,
    /// The configured wall-clock budget was exhausted.
    Timeout,
    /// No solution was found, but infeasibility was not proved.
    NotFound,
    /// The solver proved that no valid acyclic assignment exists.
    Infeasible,
    /// The solver produced an assignment that failed validation.
    InvalidSolution,
    /// Solver-specific or internal failure.
    Custom(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotCanonical => write!(f, "please canonicalize root token"),
            Self::RootMissing => write!(f, "root value is not present in the extracted tables"),
            Self::Timeout => write!(f, "solver timed out"),
            Self::NotFound => write!(f, "solver found no solution"),
            Self::Infeasible => write!(f, "solver proved no solution exists"),
            Self::InvalidSolution => write!(f, "solver produced an invalid assignment"),
            Self::Custom(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}
