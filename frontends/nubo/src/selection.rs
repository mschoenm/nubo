//! Moving the highlighted row of a menu with the D-pad. A fresh press past
//! either end wraps around to the other, while a held key's repeats stop
//! there, so holding a direction doesn't go round and round.

/// Row `selection` of `rows` moved by `step`: around the other end with
/// `wrap`, or stopped at the first or last row without it.
pub fn moved(selection: usize, step: isize, rows: usize, wrap: bool) -> usize {
    if rows == 0 {
        return 0;
    }
    let last = rows - 1;
    match selection.checked_add_signed(step) {
        Some(moved) if moved <= last => moved,
        _ if !wrap => selection.saturating_add_signed(step).min(last),
        _ if step < 0 => last,
        _ => 0,
    }
}

#[cfg(test)]
mod tests;
