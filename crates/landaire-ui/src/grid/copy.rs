use crate::grid::column::GridColumn;

/// `rows` as tab-separated text, one line per row and one field per column that has copy text.
///
/// A tab or a line break inside a field would split it, so each becomes a space.
pub(crate) fn tsv<'a, T>(rows: impl Iterator<Item = &'a T>, columns: &[&GridColumn<T>]) -> String
where
    T: 'a,
{
    let mut text = String::new();
    for row in rows {
        if !text.is_empty() {
            text.push('\n');
        }

        let fields = columns.iter().filter_map(|column| column.cell_text(row));
        for (index, field) in fields.enumerate() {
            if index > 0 {
                text.push('\t');
            }
            text.extend(field.chars().map(|character| match character {
                '\t' | '\n' | '\r' => ' ',
                other => other,
            }));
        }
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_holding_a_tab_or_a_line_break_stays_one_field() {
        let name = GridColumn::new("name", "Name", |row: &(&str, u32)| row.0.to_owned()).text(|row| row.0.to_owned());
        let count =
            GridColumn::new("count", "Count", |row: &(&str, u32)| row.1.to_string()).text(|row| row.1.to_string());
        let shown = GridColumn::new("shown", "Shown", |row: &(&str, u32)| row.0.to_owned());
        let rows = [("first\tpart", 1), ("two\nlines", 22)];

        let text = tsv(rows.iter(), &[&name, &shown, &count]);

        assert_eq!(text, "first part\t1\ntwo lines\t22");
    }
}
