//! Labels (mf_i18n) and the list diff for ListChange.
use md3_model::ListChange;

/// "1 track" / "5 tracks" (Russian has three forms: one, few, many).
pub fn tracks(count: i64) -> String {
    mf_i18n::n(count, "{0} track", "{0} tracks")
}

/// How to change `old` to get `new` without losing the scroll position: an append, one insert, one removal
/// or a swap of one row; otherwise — a reset. None — no changes.
pub fn list_change<T: Clone + PartialEq>(old: &[T], new: &[T]) -> Option<ListChange<T>> {
    if old == new {
        return None;
    }
    if old.is_empty() {
        return Some(ListChange::Reset(new.to_vec()));
    }
    if new.len() > old.len() && new.starts_with(old) {
        return Some(ListChange::Append(new[old.len()..].to_vec()));
    }
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    if new.len() > old.len() {
        let added = new.len() - old.len();
        if new[prefix + added..] == old[prefix..] {
            return Some(ListChange::Insert { at: prefix, items: new[prefix..prefix + added].to_vec() });
        }
    } else if new.len() < old.len() {
        let removed = old.len() - new.len();
        if old[prefix + removed..] == new[prefix..] {
            return Some(ListChange::Remove { at: prefix, count: removed });
        }
    } else {
        let suffix = old.iter().rev().zip(new.iter().rev()).take_while(|(a, b)| a == b).count();
        let end = old.len() - suffix;
        if end > prefix + 1 {
            let (from, to) = (prefix, end - 1);
            if old[from] == new[to] && old[from + 1..=to] == new[from..to] {
                return Some(ListChange::Move { from, to });
            }
            if old[to] == new[from] && old[from..to] == new[from + 1..=to] {
                return Some(ListChange::Move { from: to, to: from });
            }
        }
    }
    Some(ListChange::Reset(new.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_counts() {
        assert_eq!(tracks(1), "1 track");
        assert_eq!(tracks(3), "3 tracks");
        assert_eq!(tracks(0), "0 tracks");
    }

    #[test]
    fn small_edits_keep_rows() {
        assert_eq!(list_change(&[1, 2], &[1, 2]), None);
        assert_eq!(list_change(&[1, 2], &[1, 2, 3]), Some(ListChange::Append(vec![3])));
        assert_eq!(list_change(&[1, 2], &[0, 1, 2]), Some(ListChange::Insert { at: 0, items: vec![0] }));
        assert_eq!(list_change(&[1, 2, 3], &[1, 3]), Some(ListChange::Remove { at: 1, count: 1 }));
        assert_eq!(list_change(&[1, 2, 3, 4], &[2, 3, 1, 4]), Some(ListChange::Move { from: 0, to: 2 }));
        assert_eq!(list_change(&[1, 2, 3, 4], &[3, 1, 2, 4]), Some(ListChange::Move { from: 2, to: 0 }));
        assert_eq!(list_change(&[1, 2], &[3, 4]), Some(ListChange::Reset(vec![3, 4])));
    }
}
