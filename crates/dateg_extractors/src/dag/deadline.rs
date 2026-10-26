use std::time::{Duration, Instant};

use super::Error;

pub(crate) fn check_deadline(deadline: Option<Instant>) -> Result<(), Error> {
    match deadline {
        Some(deadline) if Instant::now() >= deadline => Err(Error::Timeout),
        _ => Ok(()),
    }
}

pub(crate) fn remaining(deadline: Option<Instant>) -> Result<Option<Duration>, Error> {
    match deadline {
        Some(deadline) => {
            let now = Instant::now();
            if now >= deadline {
                Err(Error::Timeout)
            } else {
                Ok(Some(deadline - now))
            }
        }
        None => Ok(None),
    }
}
