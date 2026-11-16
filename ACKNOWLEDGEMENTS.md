# Acknowledgements

WoWs Toolkit builds on the work of other people. This file credits the projects and people it borrows from. The license notices of everything the releases distribute are in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).

## People

- Trackpad, TTaro and lkolbly, for their contributions to the toolkit.
- janatan, for matching the PR colors to WoWs Numbers.

## Replay format and game data

- [wows-replays](https://github.com/lkolbly/wows-replays) by Lane Kolbly is where the replay parser and replayshark began. Its issue tracker also documents packet values the decoder relies on.
- [wows_shell](https://github.com/jcw780/wows_shell) by jcw780 supplies the shell normalization, velocity loss and fuse formulas that the ballistics code ports.
- [rust-crypto](https://github.com/DaGenix/rust-crypto) supplies the Blowfish tables and key expansion used to decrypt replays.
- [meshoptimizer](https://github.com/zeux/meshoptimizer) documents the vertex encoding the game's model files use.

## Ratings

- [WoWs Numbers](https://wows-numbers.com/personal/rating) defines the personal rating (PR) formula, its rating bands and colors, and publishes the expected values the toolkit downloads.

## User interface

- [egui](https://github.com/emilk/egui) and eframe by Emil Ernerfeldt draw the stable desktop app.
- [zgui](https://github.com/zortax/zgui) by Leonard Seibold draws the beta desktop app.
- The icons come from [Lucide](https://lucide.dev), [Phosphor Icons](https://phosphoricons.com), [Simple Icons](https://simpleicons.org) and [Font Awesome Free](https://fontawesome.com).

## Trademarks

World of Warships is a trademark of Wargaming.net. WoWs Toolkit is not affiliated with or endorsed by Wargaming.net.
