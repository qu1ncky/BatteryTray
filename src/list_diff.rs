//! How the main window brings a device list to its new state while keeping every row that stays. Slint keeps a row's
//! element only through a model's in-place changes: replacing the whole list rebuilds every element, and with them a
//! menu anchored to a card, a card being dragged, a name being typed and a switch half way through its slide.

/// One change to a list, as a model takes it.
#[derive(Clone, Debug, PartialEq)]
pub enum Step<T> {
    /// Take out the row at this index.
    Remove(usize),
    /// Put this row in at this index.
    Insert(usize, T),
    /// The row at this index stays the same element and shows this.
    Replace(usize, T),
}

/// The steps that turn `shown` into `fresh`, to be applied in order (each index is valid at the moment its step is
/// applied). Rows are matched by `key`. The row whose key is `keep` (a name being typed in it) is never taken out
/// while it is in both lists: when it has to move, the rows between move instead.
pub fn steps<T: PartialEq, K: PartialEq>(
    shown: &[T],
    fresh: Vec<T>,
    key: impl Fn(&T) -> K,
    keep: Option<K>,
) -> Vec<Step<T>> {
    let fresh_keys: Vec<K> = fresh.iter().map(&key).collect();
    let len = fresh.len();
    let (mut rest, gone): (Vec<usize>, Vec<usize>) =
        (0..shown.len()).partition(|&s| fresh_keys.contains(&key(&shown[s])));
    // Gone rows first, from the bottom up, so the indices above stay what they were
    let mut steps: Vec<Step<T>> = gone.into_iter().rev().map(Step::Remove).collect();
    // From the top down, row i of the list is made row i of the fresh one. `rest` is what is shown from index i on:
    // rows of `shown`, since every row put in so far went in above it
    for (i, (row, k)) in fresh.into_iter().zip(fresh_keys).enumerate() {
        match rest.iter().position(|&s| key(&shown[s]) == k) {
            Some(0) => {
                if shown[rest.remove(0)] != row {
                    steps.push(Step::Replace(i, row));
                }
            }
            Some(at) if keep.as_ref() != Some(&k) => {
                rest.remove(at);
                steps.push(Step::Remove(i + at));
                steps.push(Step::Insert(i, row));
            }
            // The kept row stays the element it is, so the ones above it go; each of them comes back in when the
            // fresh list gets to it
            Some(at) => {
                steps.extend((0..at).map(|_| Step::Remove(i)));
                rest.drain(..at);
                if shown[rest.remove(0)] != row {
                    steps.push(Step::Replace(i, row));
                }
            }
            None => steps.push(Step::Insert(i, row)),
        }
    }
    // Left over below the fresh rows only when `shown` named a key more often than `fresh` does
    steps.extend((0..rest.len()).rev().map(|r| Step::Remove(len + r)));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row: its key and what it shows.
    type Row = (u8, u8);

    fn diff(shown: &[Row], fresh: &[Row], keep: Option<u8>) -> Vec<Step<Row>> {
        steps(shown, fresh.to_vec(), |r| r.0, keep)
    }

    /// What a model does with the steps. A step whose index is out of range panics here, as it would in the model.
    fn apply(shown: &[Row], steps: Vec<Step<Row>>) -> Vec<Row> {
        let mut list = shown.to_vec();
        for step in steps {
            match step {
                Step::Remove(i) => {
                    list.remove(i);
                }
                Step::Insert(i, row) => list.insert(i, row),
                Step::Replace(i, row) => list[i] = row,
            }
        }
        list
    }

    /// Lists drawn by a fixed linear congruential generator: the same ones on every run, without a crate for it.
    struct Lists(u64);

    impl Lists {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }

        /// Up to 8 rows, each key at most once, in any order, each showing one of three things.
        fn list(&mut self) -> Vec<Row> {
            let mut keys: Vec<u8> = (0..8).filter(|_| self.below(3) > 0).collect();
            for i in (1..keys.len()).rev() {
                keys.swap(i, self.below(i as u64 + 1) as usize);
            }
            keys.into_iter().map(|k| (k, self.below(3) as u8)).collect()
        }

        /// A shown list, a fresh one, and the key of a shown row to keep (or none).
        fn pairs(&mut self, count: usize) -> Vec<(Vec<Row>, Vec<Row>, Option<u8>)> {
            (0..count)
                .map(|_| {
                    let (shown, fresh) = (self.list(), self.list());
                    let keep = match self.below(shown.len() as u64 + 1) as usize {
                        i if i < shown.len() => Some(shown[i].0),
                        _ => None,
                    };
                    (shown, fresh, keep)
                })
                .collect()
        }
    }

    #[test]
    fn the_steps_make_the_shown_list_the_fresh_one() {
        let pairs = Lists(1).pairs(2000);
        assert!(pairs.iter().filter(|p| p.2.is_some()).count() > 1000, "with a row to keep and without");
        for (shown, fresh, keep) in pairs {
            assert_eq!(apply(&shown, diff(&shown, &fresh, None)), fresh, "{shown:?} -> {fresh:?}");
            assert_eq!(apply(&shown, diff(&shown, &fresh, keep)), fresh, "{shown:?} -> {fresh:?} keeping {keep:?}");
        }
        // Also when a key comes twice, which a list of devices never has
        let (shown, fresh) = ([(1, 0), (2, 0), (1, 1), (2, 1)], [(2, 2), (1, 0)]);
        assert_eq!(apply(&shown, diff(&shown, &fresh, Some(1))), fresh);
        assert_eq!(apply(&fresh, diff(&fresh, &shown, Some(2))), shown);
    }

    #[test]
    fn the_row_being_typed_in_is_never_taken_out() {
        for (shown, fresh, keep) in Lists(2).pairs(2000) {
            let Some(keep) = keep.filter(|k| fresh.iter().any(|r| r.0 == *k)) else { continue };
            let mut at = shown.iter().position(|r| r.0 == keep).unwrap();
            for step in diff(&shown, &fresh, Some(keep)) {
                match step {
                    Step::Remove(i) => {
                        assert_ne!(i, at, "{shown:?} -> {fresh:?} keeping {keep}");
                        at -= usize::from(i < at);
                    }
                    Step::Insert(i, _) => at += usize::from(i <= at),
                    Step::Replace(..) => {}
                }
            }
            assert_eq!(fresh[at].0, keep, "it ends up where the fresh list has it");
        }
        // Moved up past two rows: those two go below it instead, and come back in after it
        let steps = diff(&[(1, 0), (2, 0), (3, 0)], &[(3, 1), (1, 0), (2, 0)], Some(3));
        let up = [
            Step::Remove(0),
            Step::Remove(0),
            Step::Replace(0, (3, 1)),
            Step::Insert(1, (1, 0)),
            Step::Insert(2, (2, 0)),
        ];
        assert_eq!(steps, up);
        // Moved down, it need not go anywhere: the rows above it move past it
        let steps = diff(&[(1, 0), (2, 0), (3, 0)], &[(2, 0), (3, 0), (1, 1)], Some(1));
        let down = [
            Step::Remove(1),
            Step::Insert(0, (2, 0)),
            Step::Remove(2),
            Step::Insert(1, (3, 0)),
            Step::Replace(2, (1, 1)),
        ];
        assert_eq!(steps, down);
    }

    #[test]
    fn equal_lists_take_no_steps() {
        for (shown, _, keep) in Lists(3).pairs(100) {
            assert!(diff(&shown, &shown, keep).is_empty(), "{shown:?}");
        }
        assert!(diff(&[], &[], None).is_empty());
    }

    #[test]
    fn rows_that_stay_put_are_only_replaced_and_only_when_they_changed() {
        let mut lists = Lists(4);
        for _ in 0..500 {
            let shown = lists.list();
            let fresh: Vec<Row> = shown.iter().map(|r| (r.0, lists.below(3) as u8)).collect();
            let changed: Vec<Step<Row>> =
                (0..fresh.len()).filter(|&i| fresh[i] != shown[i]).map(|i| Step::Replace(i, fresh[i])).collect();
            assert_eq!(diff(&shown, &fresh, shown.first().map(|r| r.0)), changed, "{shown:?} -> {fresh:?}");
        }
    }

    #[test]
    fn one_row_more_or_less_is_one_step() {
        let shown = [(1, 0), (2, 0), (3, 0), (4, 0)];
        for at in 0..=shown.len() {
            let mut fresh = shown.to_vec();
            fresh.insert(at, (9, 0));
            assert_eq!(diff(&shown, &fresh, Some(2)), [Step::Insert(at, (9, 0))]);
        }
        for at in 0..shown.len() {
            let mut fresh = shown.to_vec();
            fresh.remove(at);
            assert_eq!(diff(&shown, &fresh, Some(2)), [Step::Remove(at)]);
        }
    }

    #[test]
    fn a_row_that_changed_where_it_stands_is_not_taken_out() {
        // Rows come and go above and below it, and the rows around it keep their order: it stays the element it is
        let shown = [(1, 0), (2, 0), (3, 0), (4, 0)];
        let fresh = [(8, 0), (2, 0), (3, 1), (9, 0)];
        let steps = [
            Step::Remove(3),
            Step::Remove(0),
            Step::Insert(0, (8, 0)),
            Step::Replace(2, (3, 1)),
            Step::Insert(3, (9, 0)),
        ];
        assert_eq!(diff(&shown, &fresh, None), steps);
    }
}
