/// Game strings use non-breaking spaces and doubled spaces between a number
/// and its unit. Folds both to single ASCII spaces.
pub(crate) fn clean(text: &str) -> String {
    let spaced = text.replace('\u{a0}', " ");

    let mut out = String::with_capacity(spaced.len());
    let mut previous_space = false;
    for character in spaced.chars() {
        if character == ' ' && previous_space {
            continue;
        }
        previous_space = character == ' ';
        out.push(character);
    }

    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_folds_spaces() {
        assert_eq!(clean("Main Battery Modification\u{a0}1"), "Main Battery Modification 1");
        assert_eq!(clean("+450  HP increase"), "+450 HP increase");
    }
}
