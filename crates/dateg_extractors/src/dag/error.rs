use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum ExtractError {
    RootNotCanonical,
    /// The root is not present in the selected constructor tables.
    RootMissing,
    /// The solver found no valid acyclic assignment. For `Naive` this is a
    /// proof that none exists; for the heuristic and imprecise solvers it is
    /// not.
    Unsolved,
    /// The configured wall-clock budget was exhausted.
    Timeout,
    /// The solver produced an assignment that failed validation.
    InvalidSolution,
    /// Solver-specific or internal failure.
    Other(anyhow::Error),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotCanonical => write!(f, "please canonicalize root token"),
            Self::RootMissing => write!(f, "root value is not present in the extracted tables"),
            Self::Unsolved => write!(f, "solver found no solution"),
            Self::Timeout => write!(f, "solver timed out"),
            Self::InvalidSolution => write!(f, "solver produced an invalid assignment"),
            Self::Other(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ExtractError {}

pub(crate) fn check_deadline(deadline: Option<Instant>) -> Result<(), ExtractError> {
    match deadline {
        Some(deadline) if Instant::now() >= deadline => Err(ExtractError::Timeout),
        _ => Ok(()),
    }
}

pub(crate) fn remaining(deadline: Option<Instant>) -> Result<Option<Duration>, ExtractError> {
    match deadline {
        Some(deadline) => {
            let now = Instant::now();
            if now >= deadline {
                Err(ExtractError::Timeout)
            } else {
                Ok(Some(deadline - now))
            }
        }
        None => Ok(None),
    }
}
