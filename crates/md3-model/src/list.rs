/// A list change — the bridge applies it to DictListModel (begin/endInsertRows and so on).
#[derive(Clone, Debug, PartialEq)]
pub enum ListChange<T> {
    Reset(Vec<T>),
    Append(Vec<T>),
    Insert { at: usize, items: Vec<T> },
    Remove { at: usize, count: usize },
    /// moving a single row (playlist editing, the queue)
    Move { from: usize, to: usize },
    Loading(bool),
}
