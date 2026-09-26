//! Label-and-value listings, shared so a breakdown names and orders its lines
//! the same in both front ends.

use crate::formatting::separate_number;

/// One line of a breakdown: what was counted, and how much of it there was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakdownRow {
    pub label: String,
    pub value: String,
}

/// The lines a description table yields for `get`, in the table's own order.
///
/// A key with nothing behind it is left out: the tables name every damage and
/// hit type the game has, and listing the ones a battle never produced would
/// bury the handful it did.
pub fn rows<F: Fn(&str) -> u64>(descriptions: &[(&str, &str)], locale: Option<&str>, get: F) -> Vec<BreakdownRow> {
    descriptions
        .iter()
        .filter_map(|(key, description)| {
            let count = get(key);
            (count > 0)
                .then(|| BreakdownRow { label: (*description).to_owned(), value: separate_number(count, locale) })
        })
        .collect()
}

/// The lines as one block of text, each value pushed out past the longest
/// label. For a plain-text tooltip, which has no columns to line up in.
pub fn padded_text(rows: &[BreakdownRow]) -> String {
    let width = rows.iter().map(|row| row.label.len()).max().unwrap_or_default() + 1;
    rows.iter()
        .map(|row| format!("{label:<width$}: {value}", label = row.label, value = row.value))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTIONS: [(&str, &str); 3] = [("ap", "AP"), ("he", "HE"), ("fire", "Fire")];

    /// A key with nothing behind it is left out, and the rest keep the table's
    /// order rather than the order of the counts.
    #[test]
    fn only_the_types_a_battle_produced_are_listed() {
        let rows = rows(&DESCRIPTIONS, None, |key| match key {
            "ap" => 1200,
            "fire" => 34,
            _ => 0,
        });

        assert_eq!(
            rows,
            vec![
                BreakdownRow { label: "AP".to_owned(), value: "1,200".to_owned() },
                BreakdownRow { label: "Fire".to_owned(), value: "34".to_owned() },
            ]
        );
    }

    /// Every value starts at the same column, one past the longest label.
    #[test]
    fn the_text_form_pushes_every_value_past_the_longest_label() {
        let rows = rows(&DESCRIPTIONS, None, |_| 7);

        assert_eq!(padded_text(&rows), "AP   : 7\nHE   : 7\nFire : 7");
    }

    #[test]
    fn nothing_counted_is_nothing_to_read() {
        assert_eq!(padded_text(&rows(&DESCRIPTIONS, None, |_| 0)), "");
    }
}
