//! A searchable Select list whose search folds accents and matches loosely,
//! best match first, with each row marked where it matched.
//!
//! The kit's own list keeps its order and matches by a plain substring.

use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement;
use gpui_kit::SharedString;
use gpui_kit::Styled;
use gpui_kit::Task;
use gpui_kit::Window;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable;
use gpui_kit::component::h_flex;
use gpui_kit::component::searchable_list::SearchableListDelegate;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder;

use crate::text_filter::FilterKey;
use crate::text_filter::FilterQuery;
use crate::text_filter::MarkedLabel;
use crate::text_filter::MatchScore;

/// An item a [`FilteredSelect`] lists.
pub(crate) trait FilteredItem: SearchableListItem {
    /// Every text the item is found by. The row draws only its title, marked
    /// where the search matched it.
    fn search_texts(&self) -> Vec<SharedString> {
        vec![self.title()]
    }
}

/// The items of a searchable Select, each text folded once when the list is
/// built rather than on every keystroke.
pub(crate) struct FilteredSelect<T> {
    items: Vec<T>,
    keys: Vec<Box<[FilterKey]>>,

    /// Positions in `items` of the rows shown, in the order shown.
    shown: Vec<usize>,
    query: FilterQuery,
}

impl<T> FilteredSelect<T>
where
    T: FilteredItem,
{
    pub(crate) fn new(items: Vec<T>) -> Self {
        let keys =
            items.iter().map(|item| item.search_texts().iter().map(|text| FilterKey::new(text)).collect()).collect();
        let shown = (0..items.len()).collect();

        Self { items, keys, shown, query: FilterQuery::default() }
    }
}

impl<T> SearchableListDelegate for FilteredSelect<T>
where
    T: FilteredItem + 'static,
{
    type Item = T;

    fn items_count(&self, _section: usize) -> usize {
        self.shown.len()
    }

    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        self.items.get(*self.shown.get(ix.row)?)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.shown.iter().position(|&shown| self.items[shown].value() == value).map(|row| IndexPath::default().row(row))
    }

    fn perform_search(&mut self, query: &str, _window: &mut Window, _cx: &mut App) -> Task<()> {
        self.query = FilterQuery::new(query);
        if !self.query.is_active() {
            self.shown = (0..self.items.len()).collect();
            return Task::ready(());
        }

        let mut scored: Vec<(MatchScore, usize)> = self
            .keys
            .iter()
            .enumerate()
            .filter_map(|(position, keys)| Some((self.query.best(keys.iter())?.1, position)))
            .collect();
        // Stable, so items that score alike keep the order they were given.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        self.shown = scored.into_iter().map(|(_, position)| position).collect();

        Task::ready(())
    }

    /// The kit's row content and check mark, with the title marked. The kit
    /// leaves the check mark out of a row drawn here.
    fn render_item(
        &self,
        _ix: IndexPath,
        item: &Self::Item,
        checked: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        let row = h_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_between()
            .gap_x_1()
            .child(div().min_w_0().truncate().child(self.query.marked(item.title())))
            .when(checked, |row| row.child(Icon::new(IconName::Check).xsmall().flex_shrink_0()));

        Some(row.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[derive(Clone)]
    struct Place {
        name: SharedString,
        code: SharedString,
    }

    impl SearchableListItem for Place {
        type Value = SharedString;

        fn title(&self) -> SharedString {
            self.name.clone()
        }

        fn value(&self) -> &Self::Value {
            &self.name
        }
    }

    impl FilteredItem for Place {
        fn search_texts(&self) -> Vec<SharedString> {
            vec![self.name.clone(), self.code.clone()]
        }
    }

    fn places() -> FilteredSelect<Place> {
        let place = |name: &'static str, code: &'static str| Place { name: name.into(), code: code.into() };

        FilteredSelect::new(vec![
            place("Islands", "28"),
            place("Sm\u{e5}land", "41"),
            place("Land of Fire", "17"),
            place("Ocean", "16"),
        ])
    }

    fn shown(list: &FilteredSelect<Place>) -> Vec<&str> {
        (0..list.items_count(0))
            .map(|row| &*list.item(IndexPath::default().row(row)).expect("a shown row").name)
            .collect()
    }

    #[gpui_kit::test]
    fn a_search_ranks_the_matching_items_best_first(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let mut list = places();

            list.perform_search("land", window, cx).detach();
            assert_eq!(shown(&list), ["Land of Fire", "Islands", "Sm\u{e5}land"]);
            assert_eq!(list.position(&SharedString::from("Islands")), Some(IndexPath::default().row(1)));
            assert_eq!(list.position(&SharedString::from("Ocean")), None);

            list.perform_search(" ", window, cx).detach();
            assert_eq!(shown(&list), ["Islands", "Sm\u{e5}land", "Land of Fire", "Ocean"]);
        });
    }

    #[gpui_kit::test]
    fn a_search_folds_accents_and_reads_every_text_of_an_item(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let mut list = places();

            list.perform_search("smal", window, cx).detach();
            assert_eq!(shown(&list), ["Sm\u{e5}land"]);

            list.perform_search("lnd o fr", window, cx).detach();
            assert_eq!(shown(&list), ["Land of Fire"]);

            list.perform_search("16", window, cx).detach();
            assert_eq!(shown(&list), ["Ocean"]);
        });
    }

    #[gpui_kit::test]
    fn a_row_is_drawn_marked_where_its_title_matched(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let mut list = places();
            let first = IndexPath::default().row(0);

            list.perform_search("smal", window, cx).detach();
            crate::text_filter::take_marks();
            let item = list.item(first).expect("a match").clone();
            assert!(list.render_item(first, &item, true, window, cx).is_some());
            assert_eq!(crate::text_filter::marks_for("Sm\u{e5}land"), Some(std::iter::once(0..5).collect()));

            // Found by its code, which the row does not draw.
            list.perform_search("16", window, cx).detach();
            crate::text_filter::take_marks();
            let item = list.item(first).expect("a match").clone();
            assert!(list.render_item(first, &item, false, window, cx).is_some());
            assert_eq!(crate::text_filter::marks_for("Ocean"), Some(Vec::new()));
        });
    }
}
