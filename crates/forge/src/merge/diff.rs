//! A longest common subsequence of two sequences, found with Myers' greedy diff and bounded by a
//! number of edits, so that its cost stays bounded whatever the input: O((n + m) · D) time and
//! O(D²) memory for D edits at most.

/// For each element of `a`, the element of `b` it is kept as, if it is; none when turning `a`
/// into `b` takes more than `max_edits` insertions and deletions.
pub(super) fn matches<K: Eq>(a: &[K], b: &[K], max_edits: usize) -> Option<Vec<Option<usize>>> {
    let mut at = vec![None; a.len()];
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    for (i, slot) in at.iter_mut().enumerate().take(prefix) {
        *slot = Some(i);
    }
    for k in 1..=suffix {
        at[a.len() - k] = Some(b.len() - k);
    }
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    for (i, j) in middle(a_mid, b_mid, max_edits)? {
        at[prefix + i] = Some(prefix + j);
    }
    Some(at)
}

/// The pairs `(i, j)`, increasing, with `a[i] == b[j]`, of a longest common subsequence of `a`
/// and `b`; none above `max_edits` edits.
fn middle<K: Eq>(a: &[K], b: &[K], max_edits: usize) -> Option<Vec<(usize, usize)>> {
    let (n, m) = (isize::try_from(a.len()).ok()?, isize::try_from(b.len()).ok()?);
    let max = (n + m).min(isize::try_from(max_edits).unwrap_or(isize::MAX));
    if n == 0 || m == 0 {
        return (n + m <= max).then(Vec::new);
    }
    // `v[off + k]`: how far (x) the furthest path with the edits so far reaches on diagonal k.
    let off = max + 1;
    let mut v = vec![0isize; usize::try_from(2 * max + 3).ok()?];
    // Before each round d, the part of `v` it reads (diagonals -d-1 ..= d+1), to walk back.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for d in 0..=max {
        let from = usize::try_from(off - d - 1).ok()?;
        let to = usize::try_from(off + d + 1).ok()?;
        trace.push(v.get(from..=to)?.to_vec());
        for k in (-d..=d).step_by(2) {
            let idx = usize::try_from(off + k).ok()?;
            let (left, right) = (*v.get(idx - 1)?, *v.get(idx + 1)?);
            let mut x = if k == -d || (k != d && left < right) { right } else { left + 1 };
            let mut y = x - k;
            while x < n && y < m && x >= 0 && y >= 0 && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            *v.get_mut(idx)? = x;
            if x >= n && y >= m {
                return checked(a, b, back(&trace, n, m)?);
            }
        }
    }
    None
}

/// The matched pairs of the path found, walked back from the end with `trace`.
fn back(trace: &[Vec<isize>], n: isize, m: isize) -> Option<Vec<(isize, isize)>> {
    let (mut x, mut y) = (n, m);
    let mut pairs = Vec::new();
    for (d, row) in trace.iter().enumerate().rev() {
        let d = isize::try_from(d).ok()?;
        let at = |k: isize| usize::try_from(k + d + 1).ok().and_then(|i| row.get(i).copied());
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(k - 1)? < at(k + 1)?) { k + 1 } else { k - 1 };
        let prev_x = at(prev_k)?;
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y && x > 0 && y > 0 {
            x -= 1;
            y -= 1;
            pairs.push((x, y));
        }
        if d == 0 {
            break;
        }
        (x, y) = (prev_x, prev_y);
    }
    pairs.reverse();
    Some(pairs)
}

/// `pairs` as indices, if they are a common subsequence (increasing, equal elements): a check
/// that costs one pass, so that a mistake can only make a conflict, never a wrong merge.
fn checked<K: Eq>(a: &[K], b: &[K], pairs: Vec<(isize, isize)>) -> Option<Vec<(usize, usize)>> {
    let mut out = Vec::with_capacity(pairs.len());
    let mut last: Option<(usize, usize)> = None;
    for (i, j) in pairs {
        let (i, j) = (usize::try_from(i).ok()?, usize::try_from(j).ok()?);
        if a.get(i)? != b.get(j)? || last.is_some_and(|(li, lj)| li >= i || lj >= j) {
            return None;
        }
        last = Some((i, j));
        out.push((i, j));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::matches;

    fn lcs_len(a: &[u8], b: &[u8]) -> usize {
        let mut t = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                t[i][j] = if a[i] == b[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
            }
        }
        t[0][0]
    }

    /// The matching is a longest common subsequence, against the textbook table, over many
    /// small sequences from a few letters (so that they share a lot).
    #[test]
    fn the_matching_is_a_longest_common_subsequence() {
        let mut seed: u64 = 7;
        let mut next = |bound: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % bound
        };
        for _ in 0..2000 {
            let a: Vec<u8> = (0..next(12)).map(|_| b'a' + next(3) as u8).collect();
            let b: Vec<u8> = (0..next(12)).map(|_| b'a' + next(3) as u8).collect();
            let at = matches(&a, &b, usize::MAX).expect("no bound");
            let kept: Vec<(usize, usize)> = at.iter().enumerate().filter_map(|(i, j)| Some((i, (*j)?))).collect();
            assert!(kept.iter().all(|&(i, j)| a[i] == b[j]), "{a:?} {b:?}");
            assert!(kept.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1), "{a:?} {b:?}");
            assert_eq!(kept.len(), lcs_len(&a, &b), "{a:?} {b:?}");
        }
    }

    #[test]
    fn more_edits_than_allowed_is_none() {
        let (a, b) = (b"abcdefgh".as_slice(), b"ABCDEFGH".as_slice());
        assert!(matches(a, b, 15).is_none());
        assert!(matches(a, b, 16).is_some());
    }
}
