use std::time::Instant;

use anyhow::Result;

pub fn best_of<F>(warmups: usize, repeats: usize, mut f: F) -> Result<u128>
where
    F: FnMut() -> Result<()>,
{
    for _ in 0..warmups {
        f()?;
    }

    let mut best = u128::MAX;
    for _ in 0..repeats {
        let start = Instant::now();
        f()?;
        let elapsed = start.elapsed().as_millis();
        best = best.min(elapsed);
    }
    Ok(best)
}
